use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec,
    SecretKeySelector, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{Resource, ResourceExt};

use crate::crd::CloudflareTunnel;

const DEFAULT_IMAGE: &str = "cloudflare/cloudflared:2026.3.0";

/// Builds a cloudflared Deployment with volume mounts for credentials and config.
pub fn build(tunnel: &CloudflareTunnel) -> Result<Deployment, &'static str> {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .ok_or("CloudflareTunnel must be namespaced")?;
    let owner_ref = tunnel
        .controller_owner_ref(&())
        .ok_or("failed to build owner reference")?;

    let image = tunnel
        .spec
        .image
        .as_deref()
        .unwrap_or(DEFAULT_IMAGE)
        .to_string();

    let deploy_name = format!("{name}-cloudflared");
    let labels = {
        let mut l = managed_by_labels();
        l.insert("app.kubernetes.io/name".to_string(), deploy_name.clone());
        l
    };

    Ok(Deployment {
        metadata: ObjectMeta {
            name: Some(deploy_name.clone()),
            namespace: Some(namespace),
            owner_references: Some(vec![owner_ref]),
            labels: Some(managed_by_labels()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "cloudflared".to_string(),
                        image: Some(image),
                        args: Some(vec![
                            "tunnel".to_string(),
                            "--config".to_string(),
                            "/etc/cloudflared/config/config.yaml".to_string(),
                            "run".to_string(),
                        ]),
                        env: Some(vec![EnvVar {
                            name: "TUNNEL_TOKEN".to_string(),
                            value_from: Some(EnvVarSource {
                                secret_key_ref: Some(SecretKeySelector {
                                    name: format!("{name}-tunnel-credentials"),
                                    key: "token".to_string(),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }]),
                        volume_mounts: Some(vec![VolumeMount {
                            name: "config".to_string(),
                            mount_path: "/etc/cloudflared/config".to_string(),
                            read_only: Some(true),
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![Volume {
                        name: "config".to_string(),
                        config_map: Some(ConfigMapVolumeSource {
                            name: format!("{name}-config"),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
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
            "metadata": { "name": name, "namespace": ns, "uid": "uid-1" },
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
    fn deployment_has_correct_name() {
        let tunnel = test_tunnel("web", "default");
        let deploy = build(&tunnel).unwrap();
        assert_eq!(deploy.metadata.name.unwrap(), "web-cloudflared");
    }

    #[test]
    fn deployment_uses_default_image() {
        let tunnel = test_tunnel("web", "default");
        let deploy = build(&tunnel).unwrap();
        let container = &deploy.spec.unwrap().template.spec.unwrap().containers[0];
        assert_eq!(container.image.as_deref().unwrap(), DEFAULT_IMAGE);
    }

    #[test]
    fn deployment_uses_custom_image() {
        let mut tunnel = test_tunnel("web", "default");
        tunnel.spec.image = Some("cloudflare/cloudflared:2025.1.0".to_string());
        let deploy = build(&tunnel).unwrap();
        let container = &deploy.spec.unwrap().template.spec.unwrap().containers[0];
        assert_eq!(
            container.image.as_deref().unwrap(),
            "cloudflare/cloudflared:2025.1.0"
        );
    }

    #[test]
    fn deployment_mounts_config_volume() {
        let tunnel = test_tunnel("web", "default");
        let deploy = build(&tunnel).unwrap();
        let spec = deploy.spec.unwrap().template.spec.unwrap();
        let mounts = spec.containers[0].volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 1);
        assert!(mounts.iter().any(|m| m.name == "config"));
    }

    #[test]
    fn deployment_injects_tunnel_token_env() {
        let tunnel = test_tunnel("web", "default");
        let deploy = build(&tunnel).unwrap();
        let container = &deploy.spec.unwrap().template.spec.unwrap().containers[0];
        let env = container.env.as_ref().expect("env vars should be set");
        let token_env = env
            .iter()
            .find(|e| e.name == "TUNNEL_TOKEN")
            .expect("TUNNEL_TOKEN env var");
        let source = token_env.value_from.as_ref().expect("should use valueFrom");
        let secret_ref = source.secret_key_ref.as_ref().expect("should ref a secret");
        assert_eq!(secret_ref.key, "token");
        assert!(secret_ref.name.contains("tunnel-credentials"));
    }
}
