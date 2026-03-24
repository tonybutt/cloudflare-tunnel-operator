use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Resource, ResourceExt};

use crate::crd::CloudflareTunnel;

/// Builds a ConfigMap containing the cloudflared configuration.
///
/// The config includes the tunnel ID, credentials-file path, and a single
/// ingress rule pointing to the gateway service with a catch-all 404.
pub fn build(tunnel: &CloudflareTunnel, tunnel_id: &str) -> ConfigMap {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .expect("CloudflareTunnel must be namespaced");
    let owner_ref = tunnel.controller_owner_ref(&()).unwrap();

    let gateway_svc = format!("{name}-gateway.{namespace}.svc.cluster.local");

    let config = format!(
        "tunnel: {tunnel_id}\n\
         credentials-file: /etc/cloudflared/creds/credentials.json\n\
         ingress:\n\
         - service: http://{gateway_svc}\n\
         - service: http_status:404\n"
    );

    let mut data = BTreeMap::new();
    data.insert("config.yaml".to_string(), config);

    ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{name}-config")),
            namespace: Some(namespace),
            owner_references: Some(vec![owner_ref]),
            labels: Some(managed_by_labels()),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    }
}

fn managed_by_labels() -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "cloudflare-tunnel-operator".to_string(),
    );
    labels
}
