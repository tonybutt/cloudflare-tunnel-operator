use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Resource, ResourceExt};

use crate::cloudflared_config::{CloudflaredConfig, IngressRule};
use crate::crd::CloudflareTunnel;

/// Builds a ConfigMap containing the cloudflared configuration.
///
/// The config includes the tunnel ID, credentials-file path, and a single
/// ingress rule pointing to the gateway service with a catch-all 404.
pub fn build(tunnel: &CloudflareTunnel, tunnel_id: &str) -> Result<ConfigMap, &'static str> {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .ok_or("CloudflareTunnel must be namespaced")?;
    let owner_ref = tunnel
        .controller_owner_ref(&())
        .ok_or("failed to build owner reference")?;

    let gateway_svc = format!("{name}-gateway.{namespace}.svc.cluster.local");

    let config = CloudflaredConfig {
        tunnel: tunnel_id.to_string(),
        credentials_file: "/etc/cloudflared/creds/credentials.json".to_string(),
        ingress: vec![IngressRule {
            hostname: None,
            service: format!("http://{gateway_svc}"),
        }],
    };

    let config_yaml =
        serde_yaml::to_string(&config).map_err(|_| "failed to serialize cloudflared config")?;

    let mut data = BTreeMap::new();
    data.insert("config.yaml".to_string(), config_yaml);

    Ok(ConfigMap {
        metadata: ObjectMeta {
            name: Some(format!("{name}-config")),
            namespace: Some(namespace),
            owner_references: Some(vec![owner_ref]),
            labels: Some(managed_by_labels()),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    })
}

fn managed_by_labels() -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "cloudflare-tunnel-operator".to_string(),
    );
    labels
}
