mod tunnel_lifecycle;

use std::sync::Arc;

use cloudflare_tunnel_operator::cloudflare::client::{CloudflareApi, CloudflareError};
use cloudflare_tunnel_operator::cloudflare::types::{DnsRecord, Tunnel};
use cloudflare_tunnel_operator::controller;
use kube::{Client, CustomResourceExt};
use tokio::sync::Mutex;

/// Mock Cloudflare client that returns fake data for testing.
#[derive(Default)]
pub struct MockCloudflareClient {
    pub tunnels_created: Mutex<Vec<String>>,
    pub tunnels_deleted: Mutex<Vec<String>>,
    pub dns_records_created: Mutex<Vec<String>>,
    pub dns_records_deleted: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl CloudflareApi for MockCloudflareClient {
    async fn get_zone_id(&self, _zone_name: &str) -> Result<(String, String), CloudflareError> {
        Ok(("fake-zone-id".to_string(), "fake-account-id".to_string()))
    }

    async fn create_tunnel(
        &self,
        _account_id: &str,
        name: &str,
    ) -> Result<(Tunnel, String), CloudflareError> {
        let tunnel_id = format!("fake-tunnel-{name}");
        self.tunnels_created.lock().await.push(tunnel_id.clone());

        let tunnel = Tunnel {
            id: tunnel_id.clone(),
            name: name.to_string(),
        };
        let creds = serde_json::json!({
            "AccountTag": "fake-account-id",
            "TunnelID": tunnel_id,
            "TunnelSecret": "ZmFrZS1zZWNyZXQ=",
        });
        Ok((tunnel, creds.to_string()))
    }

    async fn delete_tunnel(
        &self,
        _account_id: &str,
        tunnel_id: &str,
    ) -> Result<(), CloudflareError> {
        self.tunnels_deleted
            .lock()
            .await
            .push(tunnel_id.to_string());
        Ok(())
    }

    async fn get_tunnel(
        &self,
        _account_id: &str,
        tunnel_id: &str,
    ) -> Result<Option<Tunnel>, CloudflareError> {
        let created = self.tunnels_created.lock().await;
        if created.contains(&tunnel_id.to_string()) {
            Ok(Some(Tunnel {
                id: tunnel_id.to_string(),
                name: "fake-tunnel".to_string(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn ensure_dns_cname(
        &self,
        _zone_id: &str,
        hostname: &str,
        tunnel_id: &str,
    ) -> Result<DnsRecord, CloudflareError> {
        let record_id = format!("fake-dns-{hostname}");
        self.dns_records_created
            .lock()
            .await
            .push(record_id.clone());

        Ok(DnsRecord {
            id: record_id,
            name: hostname.to_string(),
            content: format!("{tunnel_id}.cfargotunnel.com"),
            record_type: "CNAME".to_string(),
        })
    }

    async fn delete_dns_record(
        &self,
        _zone_id: &str,
        record_id: &str,
    ) -> Result<(), CloudflareError> {
        self.dns_records_deleted
            .lock()
            .await
            .push(record_id.to_string());
        Ok(())
    }

    async fn list_dns_records_by_comment(
        &self,
        _zone_id: &str,
    ) -> Result<Vec<DnsRecord>, CloudflareError> {
        Ok(vec![])
    }
}

/// Create a kind cluster, install the CRD, and return a kube Client.
pub async fn setup_kind_cluster(name: &str) -> Client {
    // Create kind cluster
    let status = tokio::process::Command::new("kind")
        .args(["create", "cluster", "--name", name, "--wait", "60s"])
        .status()
        .await
        .expect("failed to run kind");
    assert!(status.success(), "kind create cluster failed");

    // Get kubeconfig for this cluster
    let output = tokio::process::Command::new("kind")
        .args(["get", "kubeconfig", "--name", name])
        .output()
        .await
        .expect("failed to get kind kubeconfig");
    assert!(output.status.success(), "kind get kubeconfig failed");

    let kubeconfig = String::from_utf8(output.stdout).expect("invalid kubeconfig utf8");

    // Build kube client from the kubeconfig
    let config = kube::Config::from_custom_kubeconfig(
        kube::config::Kubeconfig::from_yaml(&kubeconfig).expect("invalid kubeconfig"),
        &kube::config::KubeConfigOptions::default(),
    )
    .await
    .expect("failed to build kube config");

    let client = Client::try_from(config).expect("failed to create kube client");

    // Install the CRD
    let crd = cloudflare_tunnel_operator::crd::CloudflareTunnel::crd();
    let crd_api: kube::Api<k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition> =
        kube::Api::all(client.clone());
    let pp = kube::api::PatchParams::apply("e2e-test").force();
    crd_api
        .patch(
            "cloudflaretunnels.tunnels.abutt.dev",
            &pp,
            &kube::api::Patch::Apply(&crd),
        )
        .await
        .expect("failed to install CRD");

    // Wait for CRD to be established
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    client
}

/// Delete a kind cluster.
pub async fn teardown_kind_cluster(name: &str) {
    let _ = tokio::process::Command::new("kind")
        .args(["delete", "cluster", "--name", name])
        .status()
        .await;
}

/// Start the controller in a background tokio task.
///
/// Returns a JoinHandle that can be aborted to stop the controller.
pub fn start_controller(
    client: Client,
    mock_cf: Arc<MockCloudflareClient>,
) -> tokio::task::JoinHandle<()> {
    let ctx = controller::Ctx {
        client,
        cf: Box::new(MockCloudflareClientWrapper(mock_cf)),
    };
    tokio::spawn(async move {
        controller::run_no_gateway(ctx).await;
    })
}

/// Wrapper to implement CloudflareApi for Arc<MockCloudflareClient>.
struct MockCloudflareClientWrapper(Arc<MockCloudflareClient>);

#[async_trait::async_trait]
impl CloudflareApi for MockCloudflareClientWrapper {
    async fn get_zone_id(&self, zone_name: &str) -> Result<(String, String), CloudflareError> {
        self.0.get_zone_id(zone_name).await
    }
    async fn create_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> Result<(Tunnel, String), CloudflareError> {
        self.0.create_tunnel(account_id, name).await
    }
    async fn delete_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<(), CloudflareError> {
        self.0.delete_tunnel(account_id, tunnel_id).await
    }
    async fn get_tunnel(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<Option<Tunnel>, CloudflareError> {
        self.0.get_tunnel(account_id, tunnel_id).await
    }
    async fn ensure_dns_cname(
        &self,
        zone_id: &str,
        hostname: &str,
        tunnel_id: &str,
    ) -> Result<DnsRecord, CloudflareError> {
        self.0.ensure_dns_cname(zone_id, hostname, tunnel_id).await
    }
    async fn delete_dns_record(
        &self,
        zone_id: &str,
        record_id: &str,
    ) -> Result<(), CloudflareError> {
        self.0.delete_dns_record(zone_id, record_id).await
    }
    async fn list_dns_records_by_comment(
        &self,
        zone_id: &str,
    ) -> Result<Vec<DnsRecord>, CloudflareError> {
        self.0.list_dns_records_by_comment(zone_id).await
    }
}
