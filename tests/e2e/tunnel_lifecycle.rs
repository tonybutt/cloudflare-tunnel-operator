use std::sync::Arc;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::api::{Api, DeleteParams, PostParams};

use cloudflare_tunnel_operator::crd::CloudflareTunnel;

use crate::{MockCloudflareClient, setup_kind_cluster, start_controller, teardown_kind_cluster};

fn test_tunnel_cr(name: &str, namespace: &str) -> CloudflareTunnel {
    serde_json::from_value(serde_json::json!({
        "apiVersion": "tunnels.abutt.dev/v1alpha1",
        "kind": "CloudflareTunnel",
        "metadata": {
            "name": name,
            "namespace": namespace,
        },
        "spec": {
            "zone": "example.com",
            "gateway": {
                "gatewayClassName": "cilium",
                "listeners": [
                    { "hostname": "app.example.com" }
                ]
            }
        }
    }))
    .expect("failed to build test CloudflareTunnel CR")
}

/// Wait for a namespaced resource to exist, with a timeout.
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

/// Wait for a namespaced resource to be deleted, with a timeout.
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

#[tokio::test]
#[ignore] // Requires kind to be available
async fn test_tunnel_create_produces_child_resources() {
    let cluster_name = "cft-e2e-create";
    let client = setup_kind_cluster(cluster_name).await;

    let mock_cf = Arc::new(MockCloudflareClient::default());
    let controller_handle = start_controller(client.clone(), mock_cf.clone());

    let ns = "default";
    let tunnel_api: Api<CloudflareTunnel> = Api::namespaced(client.clone(), ns);
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), ns);
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), ns);
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), ns);

    // Create the CloudflareTunnel CR
    let cr = test_tunnel_cr("test-tunnel", ns);
    tunnel_api
        .create(&PostParams::default(), &cr)
        .await
        .expect("failed to create CloudflareTunnel CR");

    // Wait for child resources to be created
    assert!(
        wait_for_resource(&secret_api, "test-tunnel-tunnel-credentials", 30).await,
        "Secret was not created within timeout"
    );
    assert!(
        wait_for_resource(&cm_api, "test-tunnel-config", 30).await,
        "ConfigMap was not created within timeout"
    );
    assert!(
        wait_for_resource(&deploy_api, "test-tunnel-cloudflared", 30).await,
        "Deployment was not created within timeout"
    );

    // Verify status was updated with tunnel ID
    let updated = tunnel_api
        .get("test-tunnel")
        .await
        .expect("failed to get updated CR");
    if let Some(status) = &updated.status {
        assert!(
            status.tunnel_id.is_some(),
            "tunnel_id should be set in status"
        );
        assert!(
            status
                .tunnel_id
                .as_ref()
                .unwrap()
                .starts_with("fake-tunnel-"),
            "tunnel_id should be from mock"
        );
    }

    // Verify mock was called
    let created = mock_cf.tunnels_created.lock().await;
    assert!(
        !created.is_empty(),
        "mock create_tunnel should have been called"
    );

    let dns_created = mock_cf.dns_records_created.lock().await;
    assert!(
        !dns_created.is_empty(),
        "mock ensure_dns_cname should have been called"
    );

    // Cleanup
    controller_handle.abort();
    teardown_kind_cluster(cluster_name).await;
}

#[tokio::test]
#[ignore] // Requires kind to be available
async fn test_tunnel_delete_cleans_up() {
    let cluster_name = "cft-e2e-delete";
    let client = setup_kind_cluster(cluster_name).await;

    let mock_cf = Arc::new(MockCloudflareClient::default());
    let controller_handle = start_controller(client.clone(), mock_cf.clone());

    let ns = "default";
    let tunnel_api: Api<CloudflareTunnel> = Api::namespaced(client.clone(), ns);
    let secret_api: Api<Secret> = Api::namespaced(client.clone(), ns);
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), ns);
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), ns);

    // Create the CloudflareTunnel CR
    let cr = test_tunnel_cr("test-tunnel-del", ns);
    tunnel_api
        .create(&PostParams::default(), &cr)
        .await
        .expect("failed to create CloudflareTunnel CR");

    // Wait for child resources to exist
    assert!(
        wait_for_resource(&secret_api, "test-tunnel-del-tunnel-credentials", 30).await,
        "Secret was not created within timeout"
    );
    assert!(
        wait_for_resource(&deploy_api, "test-tunnel-del-cloudflared", 30).await,
        "Deployment was not created within timeout"
    );

    // Delete the CR
    tunnel_api
        .delete("test-tunnel-del", &DeleteParams::default())
        .await
        .expect("failed to delete CloudflareTunnel CR");

    // Wait for child resources to be garbage collected
    // Owner references cause cascading deletion
    assert!(
        wait_for_deletion(&secret_api, "test-tunnel-del-tunnel-credentials", 30).await,
        "Secret was not garbage collected within timeout"
    );
    assert!(
        wait_for_deletion(&cm_api, "test-tunnel-del-config", 30).await,
        "ConfigMap was not garbage collected within timeout"
    );
    assert!(
        wait_for_deletion(&deploy_api, "test-tunnel-del-cloudflared", 30).await,
        "Deployment was not garbage collected within timeout"
    );

    // Verify mock cleanup was called
    let deleted = mock_cf.tunnels_deleted.lock().await;
    assert!(
        !deleted.is_empty(),
        "mock delete_tunnel should have been called during cleanup"
    );

    // Cleanup
    controller_handle.abort();
    teardown_kind_cluster(cluster_name).await;
}
