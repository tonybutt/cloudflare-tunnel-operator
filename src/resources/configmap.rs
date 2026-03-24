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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::*;

    fn test_tunnel(name: &str, ns: &str) -> CloudflareTunnel {
        serde_json::from_value(serde_json::json!({
            "apiVersion": "tunnels.abutt.dev/v1alpha1",
            "kind": "CloudflareTunnel",
            "metadata": { "name": name, "namespace": ns, "uid": "test-uid" },
            "spec": {
                "zone": "example.com",
                "gateway": {
                    "gateway_class_name": "cilium",
                    "listeners": [{"hostname": "app.example.com"}]
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn configmap_has_correct_name() {
        let tunnel = test_tunnel("web", "prod");
        let cm = build(&tunnel, "tunnel-123").unwrap();
        assert_eq!(cm.metadata.name.unwrap(), "web-config");
    }

    #[test]
    fn configmap_contains_tunnel_id() {
        let tunnel = test_tunnel("web", "default");
        let cm = build(&tunnel, "abc-123").unwrap();
        let data = cm.data.unwrap();
        let config = &data["config.yaml"];
        assert!(config.contains("tunnel: abc-123"));
    }

    #[test]
    fn configmap_points_to_gateway_service() {
        let tunnel = test_tunnel("web", "prod");
        let cm = build(&tunnel, "tid").unwrap();
        let data = cm.data.unwrap();
        let config = &data["config.yaml"];
        assert!(config.contains("http://web-gateway.prod.svc.cluster.local"));
    }

    #[test]
    fn configmap_has_credentials_path() {
        let tunnel = test_tunnel("t", "default");
        let cm = build(&tunnel, "tid").unwrap();
        let data = cm.data.unwrap();
        let config = &data["config.yaml"];
        assert!(config.contains("credentials-file: /etc/cloudflared/creds/credentials.json"));
    }
}
