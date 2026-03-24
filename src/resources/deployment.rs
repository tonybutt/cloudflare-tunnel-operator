use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, PodSpec, PodTemplateSpec, SecretVolumeSource, Volume,
    VolumeMount,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use kube::{Resource, ResourceExt};

use crate::crd::CloudflareTunnel;

const DEFAULT_IMAGE: &str = "cloudflare/cloudflared:2024.11.0";

/// Builds a cloudflared Deployment with volume mounts for credentials and config.
pub fn build(tunnel: &CloudflareTunnel) -> Deployment {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .expect("CloudflareTunnel must be namespaced");
    let owner_ref = tunnel.controller_owner_ref(&()).unwrap();

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

    Deployment {
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
                        volume_mounts: Some(vec![
                            VolumeMount {
                                name: "creds".to_string(),
                                mount_path: "/etc/cloudflared/creds".to_string(),
                                read_only: Some(true),
                                ..Default::default()
                            },
                            VolumeMount {
                                name: "config".to_string(),
                                mount_path: "/etc/cloudflared/config".to_string(),
                                read_only: Some(true),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }],
                    volumes: Some(vec![
                        Volume {
                            name: "creds".to_string(),
                            secret: Some(SecretVolumeSource {
                                secret_name: Some(format!("{name}-tunnel-credentials")),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                        Volume {
                            name: "config".to_string(),
                            config_map: Some(ConfigMapVolumeSource {
                                name: format!("{name}-config"),
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
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
