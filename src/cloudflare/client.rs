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
    #[error("HTTP request to {endpoint} failed: {source}")]
    Http {
        #[source]
        source: reqwest::Error,
        endpoint: String,
    },
    #[error("zone '{zone}' not found")]
    ZoneNotFound { zone: String },
    #[error("tunnel '{name}' already exists (conflict)")]
    TunnelConflict { name: String },
    #[error("failed to deserialize response from {endpoint}: {source}")]
    Deserialize {
        #[source]
        source: reqwest::Error,
        endpoint: String,
    },
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
        let endpoint = format!("{CF_API_BASE}/zones");
        let resp = self
            .http
            .get(&endpoint)
            .header("Authorization", self.auth_header())
            .query(&[("name", zone_name)])
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?;

        let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

        let body: CfListResponse<Zone> =
            resp.json()
                .await
                .map_err(|e| CloudflareError::Deserialize {
                    source: e,
                    endpoint: endpoint.clone(),
                })?;

        let zone = body
            .result
            .into_iter()
            .next()
            .ok_or_else(|| CloudflareError::ZoneNotFound {
                zone: zone_name.to_string(),
            })?;

        Ok((zone.id, zone.account.id))
    }

    async fn create_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> Result<(Tunnel, String), CloudflareError> {
        let secret_bytes: [u8; 32] = rand::rng().random();
        let tunnel_secret = base64::engine::general_purpose::STANDARD.encode(secret_bytes);

        let endpoint = format!("{CF_API_BASE}/accounts/{account_id}/cfd_tunnel");
        let resp = self
            .http
            .post(&endpoint)
            .header("Authorization", self.auth_header())
            .json(&CreateTunnelRequest {
                name: name.to_string(),
                tunnel_secret: tunnel_secret.clone(),
                config_src: "local".to_string(),
            })
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?;

        let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

        let body: CfResponse<Tunnel> =
            resp.json()
                .await
                .map_err(|e| CloudflareError::Deserialize {
                    source: e,
                    endpoint,
                })?;

        let creds = serde_json::json!({
            "AccountTag": account_id,
            "TunnelID": body.result.id,
            "TunnelSecret": tunnel_secret,
        });

        Ok((body.result, creds.to_string()))
    }

    async fn delete_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<(), CloudflareError> {
        let endpoint =
            format!("{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}/connections");
        self.http
            .delete(&endpoint)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?
            .error_for_status()
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint,
            })?;

        let endpoint = format!("{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}");
        self.http
            .delete(&endpoint)
            .header("Authorization", self.auth_header())
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?
            .error_for_status()
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint,
            })?;

        Ok(())
    }

    async fn get_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<Option<Tunnel>, CloudflareError> {
        let endpoint = format!("{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}");
        let resp = self
            .http
            .get(&endpoint)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

        let body: CfResponse<Tunnel> =
            resp.json()
                .await
                .map_err(|e| CloudflareError::Deserialize {
                    source: e,
                    endpoint,
                })?;

        Ok(Some(body.result))
    }

    async fn ensure_dns_cname(
        &self,
        zone_id: &str,
        hostname: &str,
        tunnel_id: &str,
    ) -> Result<DnsRecord, CloudflareError> {
        let target = format!("{tunnel_id}.cfargotunnel.com");

        let endpoint = format!("{CF_API_BASE}/zones/{zone_id}/dns_records");
        let resp = self
            .http
            .get(&endpoint)
            .header("Authorization", self.auth_header())
            .query(&[("type", "CNAME"), ("name", hostname)])
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?;

        let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

        let existing: CfListResponse<DnsRecord> =
            resp.json()
                .await
                .map_err(|e| CloudflareError::Deserialize {
                    source: e,
                    endpoint: endpoint.clone(),
                })?;

        if let Some(record) = existing.result.into_iter().next() {
            if record.content == target {
                return Ok(record);
            }
            self.delete_dns_record(zone_id, &record.id).await?;
        }

        let resp = self
            .http
            .post(&endpoint)
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
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?;

        let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

        let body: CfResponse<DnsRecord> =
            resp.json()
                .await
                .map_err(|e| CloudflareError::Deserialize {
                    source: e,
                    endpoint,
                })?;

        Ok(body.result)
    }

    async fn delete_dns_record(
        &self,
        zone_id: &str,
        record_id: &str,
    ) -> Result<(), CloudflareError> {
        let endpoint = format!("{CF_API_BASE}/zones/{zone_id}/dns_records/{record_id}");
        self.http
            .delete(&endpoint)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?
            .error_for_status()
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint,
            })?;

        Ok(())
    }

    async fn list_dns_records_by_comment(
        &self,
        zone_id: &str,
    ) -> Result<Vec<DnsRecord>, CloudflareError> {
        let endpoint = format!("{CF_API_BASE}/zones/{zone_id}/dns_records");
        let resp = self
            .http
            .get(&endpoint)
            .header("Authorization", self.auth_header())
            .query(&[("type", "CNAME"), ("comment.contains", TUNNEL_COMMENT)])
            .send()
            .await
            .map_err(|e| CloudflareError::Http {
                source: e,
                endpoint: endpoint.clone(),
            })?;

        let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

        let body: CfListResponse<DnsRecord> =
            resp.json()
                .await
                .map_err(|e| CloudflareError::Deserialize {
                    source: e,
                    endpoint,
                })?;

        Ok(body.result)
    }
}
