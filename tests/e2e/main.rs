mod tunnel_lifecycle;

use cloudflare_tunnel_operator::cloudflare::client::{CloudflareApi, CloudflareClient};
use cloudflare_tunnel_operator::controller;
use kube::Client;

const TEST_ZONE: &str = "anthonybutt.software";

/// Read CF_API_TOKEN from environment, panic if not set.
pub fn cf_token() -> String {
    std::env::var("CF_API_TOKEN").expect("CF_API_TOKEN must be set for e2e tests")
}

/// Create a kind cluster, install the CRD, and return a kube Client.
pub async fn setup_kind_cluster(name: &str) -> Client {
    // Delete any leftover cluster from a previous failed run
    let _ = tokio::process::Command::new("kind")
        .args(["delete", "cluster", "--name", name])
        .status()
        .await;

    let status = tokio::process::Command::new("kind")
        .args(["create", "cluster", "--name", name, "--wait", "60s"])
        .status()
        .await
        .expect("failed to run kind");
    assert!(status.success(), "kind create cluster failed");

    let output = tokio::process::Command::new("kind")
        .args(["get", "kubeconfig", "--name", name])
        .output()
        .await
        .expect("failed to get kind kubeconfig");
    assert!(output.status.success(), "kind get kubeconfig failed");

    let kubeconfig = String::from_utf8(output.stdout).expect("invalid kubeconfig utf8");

    let config = kube::Config::from_custom_kubeconfig(
        kube::config::Kubeconfig::from_yaml(&kubeconfig).expect("invalid kubeconfig"),
        &kube::config::KubeConfigOptions::default(),
    )
    .await
    .expect("failed to build kube config");

    let client = Client::try_from(config).expect("failed to create kube client");

    // Install the CRD
    use kube::CustomResourceExt;
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

/// Start the controller in a background tokio task with the real Cloudflare client.
pub fn start_controller(client: Client, token: &str) -> tokio::task::JoinHandle<()> {
    // Init tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("cloudflare_tunnel_operator=debug,kube=info")
        .with_test_writer()
        .try_init();

    let cf = CloudflareClient::new(token.to_string());
    let ctx = controller::Ctx {
        client,
        cf: Box::new(cf),
    };
    tokio::spawn(async move {
        controller::run_no_gateway(ctx).await;
    })
}

/// Clean up any tunnels and DNS records created during a test.
///
/// This is a safety net — the controller's finalizer should handle cleanup,
/// but if the test fails mid-way, this ensures we don't leave resources behind.
pub async fn cleanup_cloudflare(token: &str, tunnel_name_prefix: &str) {
    let cf = CloudflareClient::new(token.to_string());

    // Resolve zone
    let zone_result = cf.get_zone_id(TEST_ZONE).await;
    let (zone_id, account_id) = match zone_result {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("cleanup: failed to resolve zone: {e}");
            return;
        }
    };

    // Delete any DNS records managed by the operator
    match cf.list_dns_records_by_comment(&zone_id).await {
        Ok(records) => {
            for record in records {
                if record.name.contains(tunnel_name_prefix)
                    || record.content.contains("cfargotunnel.com")
                {
                    eprintln!("cleanup: deleting DNS record {}", record.name);
                    let _ = cf.delete_dns_record(&zone_id, &record.id).await;
                }
            }
        }
        Err(e) => eprintln!("cleanup: failed to list DNS records: {e}"),
    }

    // List and delete any tunnels with the test prefix
    // Use the raw HTTP client since CloudflareClient doesn't have a list method
    let http = reqwest::Client::new();
    let resp = http
        .get(format!(
            "https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("name", tunnel_name_prefix), ("is_deleted", "false")])
        .send()
        .await;

    if let Ok(resp) = resp {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(tunnels) = body["result"].as_array() {
                for tunnel in tunnels {
                    if let Some(id) = tunnel["id"].as_str() {
                        let name = tunnel["name"].as_str().unwrap_or("unknown");
                        eprintln!("cleanup: deleting tunnel {name} ({id})");
                        let _ = cf.delete_tunnel(&account_id, id).await;
                    }
                }
            }
        }
    }
}
