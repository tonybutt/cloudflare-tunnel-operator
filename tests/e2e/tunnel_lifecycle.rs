use cloudflare_tunnel_operator::cloudflare::client::{CloudflareApi, CloudflareClient};
use cloudflare_tunnel_operator::crd::CloudflareTunnel;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::api::{Api, DeleteParams, PostParams};

use crate::{
    TEST_ZONE, cf_token, cleanup_cloudflare, setup_kind_cluster, start_controller,
    teardown_kind_cluster,
};

fn test_tunnel_cr(name: &str, namespace: &str) -> CloudflareTunnel {
    serde_json::from_value(serde_json::json!({
        "apiVersion": "tunnels.abutt.dev/v1alpha1",
        "kind": "CloudflareTunnel",
        "metadata": {
            "name": name,
            "namespace": namespace,
        },
        "spec": {
            "zone": TEST_ZONE,
            "gateway": {
                "gateway_class_name": "cilium",
                "listeners": [
                    { "hostname": format!("e2e-test.{TEST_ZONE}") }
                ]
            }
        }
    }))
    .expect("failed to build test CloudflareTunnel CR")
}

async fn wait_for_resource<K>(api: &Api<K>, name: &str, timeout_secs: u64) -> bool
where
    K: kube::Resource
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + 'static,
{
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        match api.get_opt(name).await {
            Ok(Some(_)) => return true,
            _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        }
    }
}

async fn wait_for_deletion<K>(api: &Api<K>, name: &str, timeout_secs: u64) -> bool
where
    K: kube::Resource
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + 'static,
{
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        match api.get_opt(name).await {
            Ok(None) => return true,
            _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        }
    }
}

/// Verify the tunnel was actually created in Cloudflare.
async fn verify_tunnel_exists(token: &str, tunnel_name: &str) -> Option<String> {
    let cf = CloudflareClient::new(token.to_string());
    let (_, account_id) = cf.get_zone_id(TEST_ZONE).await.ok()?;

    let http = reqwest::Client::new();
    let resp = http
        .get(format!(
            "https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("name", tunnel_name), ("is_deleted", "false")])
        .send()
        .await
        .ok()?;

    let body: serde_json::Value = resp.json().await.ok()?;
    let tunnels = body["result"].as_array()?;
    tunnels
        .first()
        .and_then(|t| t["id"].as_str().map(String::from))
}

/// Verify a DNS CNAME record exists in Cloudflare.
async fn verify_dns_record_exists(token: &str, hostname: &str) -> bool {
    let cf = CloudflareClient::new(token.to_string());
    let (zone_id, _) = match cf.get_zone_id(TEST_ZONE).await {
        Ok(ids) => ids,
        Err(_) => return false,
    };

    let http = reqwest::Client::new();
    let resp = http
        .get(format!(
            "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("type", "CNAME"), ("name", hostname)])
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                body["result"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Verify a DNS record does NOT exist.
async fn verify_dns_record_gone(token: &str, hostname: &str) -> bool {
    !verify_dns_record_exists(token, hostname).await
}

/// Verify a tunnel was deleted (not found or marked deleted).
async fn verify_tunnel_gone(token: &str, tunnel_name: &str) -> bool {
    verify_tunnel_exists(token, tunnel_name).await.is_none()
}

#[tokio::test]
#[ignore]
async fn test_tunnel_full_lifecycle() {
    let cluster_name = "cft-e2e-lifecycle";
    let token = cf_token();
    let tunnel_name = "e2e-lifecycle";
    let test_hostname = format!("e2e-test.{TEST_ZONE}");

    // Safety net: clean up any leftover resources from a previous failed run
    cleanup_cloudflare(&token, tunnel_name).await;

    let client = setup_kind_cluster(cluster_name).await;
    let controller_handle = start_controller(client.clone(), &token);

    let ns = "default";
    let tunnel_api: Api<CloudflareTunnel> = Api::namespaced(client.clone(), ns);
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), ns);
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), ns);
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), ns);

    // === CREATE ===
    eprintln!("--- Creating CloudflareTunnel CR ---");
    let cr = test_tunnel_cr(tunnel_name, ns);
    tunnel_api
        .create(&PostParams::default(), &cr)
        .await
        .expect("failed to create CloudflareTunnel CR");

    // Wait for K8s child resources
    assert!(
        wait_for_resource(
            &secret_api,
            &format!("{tunnel_name}-tunnel-credentials"),
            60
        )
        .await,
        "Secret was not created within timeout"
    );
    assert!(
        wait_for_resource(&cm_api, &format!("{tunnel_name}-config"), 60).await,
        "ConfigMap was not created within timeout"
    );
    assert!(
        wait_for_resource(&deploy_api, &format!("{tunnel_name}-cloudflared"), 60).await,
        "Deployment was not created within timeout"
    );

    // Wait for status to be populated with tunnel_id
    let tunnel_id = {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if tokio::time::Instant::now() > deadline {
                panic!("status.tunnelId was not set within timeout");
            }
            let cr = tunnel_api.get(tunnel_name).await.expect("failed to get CR");
            if let Some(ref status) = cr.status {
                if let Some(ref tid) = status.tunnel_id {
                    break tid.clone();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    };

    let updated = tunnel_api
        .get(tunnel_name)
        .await
        .expect("failed to get updated CR");
    let status = updated.status.as_ref().expect("status should be set");
    eprintln!("Tunnel created with ID: {tunnel_id}");

    assert!(
        !status.routes.is_empty(),
        "routes should be populated in status"
    );
    assert_eq!(status.routes[0].hostname, test_hostname);

    // Verify Cloudflare resources were actually created
    let cf_tunnel_id = verify_tunnel_exists(&token, tunnel_name).await;
    assert!(
        cf_tunnel_id.is_some(),
        "tunnel should exist in Cloudflare account"
    );
    assert_eq!(cf_tunnel_id.as_deref(), Some(tunnel_id.as_str()));

    assert!(
        verify_dns_record_exists(&token, &test_hostname).await,
        "DNS CNAME record should exist in Cloudflare"
    );

    // === DELETE ===
    eprintln!("--- Deleting CloudflareTunnel CR ---");
    tunnel_api
        .delete(tunnel_name, &DeleteParams::default())
        .await
        .expect("failed to delete CloudflareTunnel CR");

    // Wait for K8s child resources to be garbage collected
    assert!(
        wait_for_deletion(
            &secret_api,
            &format!("{tunnel_name}-tunnel-credentials"),
            60
        )
        .await,
        "Secret was not garbage collected within timeout"
    );
    assert!(
        wait_for_deletion(&cm_api, &format!("{tunnel_name}-config"), 60).await,
        "ConfigMap was not garbage collected within timeout"
    );
    assert!(
        wait_for_deletion(&deploy_api, &format!("{tunnel_name}-cloudflared"), 60).await,
        "Deployment was not garbage collected within timeout"
    );

    // Wait a moment for the finalizer to clean up Cloudflare resources
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // Verify Cloudflare resources were cleaned up
    assert!(
        verify_tunnel_gone(&token, tunnel_name).await,
        "tunnel should be deleted from Cloudflare"
    );
    assert!(
        verify_dns_record_gone(&token, &test_hostname).await,
        "DNS record should be deleted from Cloudflare"
    );

    eprintln!("--- All assertions passed ---");

    // Cleanup
    controller_handle.abort();
    teardown_kind_cluster(cluster_name).await;

    // Final safety net cleanup
    cleanup_cloudflare(&token, tunnel_name).await;
}
