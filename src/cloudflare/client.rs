use crate::cloudflare::types::*;
use base64::Engine;
use rand::Rng;

const CF_API_BASE: &str = "https://api.cloudflare.com/client/v4";
const TUNNEL_COMMENT: &str = "Managed by cloudflare-tunnel-operator";

#[async_trait::async_trait]
pub trait CloudflareApi: Send + Sync {
    async fn get_zone_id(&self, zone_name: &str) -> Result<(String, String), CloudflareError>;
    async fn create_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> Result<(Tunnel, String), CloudflareError>;
    async fn delete_tunnel(&self, account_id: &str, tunnel_id: &str)
    -> Result<(), CloudflareError>;
    async fn get_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<Option<Tunnel>, CloudflareError>;
    async fn ensure_dns_cname(
        &self,
        zone_id: &str,
        hostname: &str,
        tunnel_id: &str,
    ) -> Result<DnsRecord, CloudflareError>;
    async fn delete_dns_record(
        &self,
        zone_id: &str,
        record_id: &str,
    ) -> Result<(), CloudflareError>;
    async fn list_dns_records_by_comment(
        &self,
        zone_id: &str,
    ) -> Result<Vec<DnsRecord>, CloudflareError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CloudflareError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
}

#[derive(Clone)]
pub struct CloudflareClient {
    http: reqwest::Client,
    token: String,
}

impl CloudflareClient {
    pub fn new(token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
        }
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

#[async_trait::async_trait]
impl CloudflareApi for CloudflareClient {
    async fn get_zone_id(&self, zone_name: &str) -> Result<(String, String), CloudflareError> {
        let resp: CfListResponse<Zone> = self
            .http
            .get(format!("{CF_API_BASE}/zones"))
            .header("Authorization", self.auth_header())
            .query(&[("name", zone_name)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let zone = resp
            .result
            .into_iter()
            .next()
            .ok_or_else(|| CloudflareError::Api(format!("zone '{}' not found", zone_name)))?;

        Ok((zone.id, zone.account.id))
    }

    async fn create_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> Result<(Tunnel, String), CloudflareError> {
        let secret_bytes: [u8; 32] = rand::rng().random();
        let tunnel_secret = base64::engine::general_purpose::STANDARD.encode(secret_bytes);

        let resp: CfResponse<Tunnel> = self
            .http
            .post(format!("{CF_API_BASE}/accounts/{account_id}/cfd_tunnel"))
            .header("Authorization", self.auth_header())
            .json(&CreateTunnelRequest {
                name: name.to_string(),
                tunnel_secret: tunnel_secret.clone(),
                config_src: "local".to_string(),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let creds = serde_json::json!({
            "AccountTag": account_id,
            "TunnelID": resp.result.id,
            "TunnelSecret": tunnel_secret,
        });

        Ok((resp.result, creds.to_string()))
    }

    async fn delete_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<(), CloudflareError> {
        self.http
            .delete(format!(
                "{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}/connections"
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await?
            .error_for_status()?;

        self.http
            .delete(format!(
                "{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}"
            ))
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn get_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<Option<Tunnel>, CloudflareError> {
        let resp = self
            .http
            .get(format!(
                "{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}"
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let body: CfResponse<Tunnel> = resp.error_for_status()?.json().await?;
        Ok(Some(body.result))
    }

    async fn ensure_dns_cname(
        &self,
        zone_id: &str,
        hostname: &str,
        tunnel_id: &str,
    ) -> Result<DnsRecord, CloudflareError> {
        let target = format!("{tunnel_id}.cfargotunnel.com");

        let existing: CfListResponse<DnsRecord> = self
            .http
            .get(format!("{CF_API_BASE}/zones/{zone_id}/dns_records"))
            .header("Authorization", self.auth_header())
            .query(&[("type", "CNAME"), ("name", hostname)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if let Some(record) = existing.result.into_iter().next() {
            if record.content == target {
                return Ok(record);
            }
            self.delete_dns_record(zone_id, &record.id).await?;
        }

        let resp: CfResponse<DnsRecord> = self
            .http
            .post(format!("{CF_API_BASE}/zones/{zone_id}/dns_records"))
            .header("Authorization", self.auth_header())
            .json(&CreateDnsRecordRequest {
                record_type: "CNAME".to_string(),
                name: hostname.to_string(),
                content: target,
                proxied: true,
                comment: TUNNEL_COMMENT.to_string(),
                ttl: 1,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp.result)
    }

    async fn delete_dns_record(
        &self,
        zone_id: &str,
        record_id: &str,
    ) -> Result<(), CloudflareError> {
        self.http
            .delete(format!(
                "{CF_API_BASE}/zones/{zone_id}/dns_records/{record_id}"
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }

    async fn list_dns_records_by_comment(
        &self,
        zone_id: &str,
    ) -> Result<Vec<DnsRecord>, CloudflareError> {
        let resp: CfListResponse<DnsRecord> = self
            .http
            .get(format!("{CF_API_BASE}/zones/{zone_id}/dns_records"))
            .header("Authorization", self.auth_header())
            .query(&[("type", "CNAME"), ("comment.contains", TUNNEL_COMMENT)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(resp.result)
    }
}
