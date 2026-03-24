use std::collections::BTreeMap;

use k8s_openapi::ByteString;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::{Resource, ResourceExt};

use crate::crd::CloudflareTunnel;

/// Builds a Secret containing tunnel credentials JSON.
///
/// The secret is named `{name}-tunnel-credentials` and stores the credentials
/// under the key `credentials.json`.
pub fn build(tunnel: &CloudflareTunnel, credentials_json: &[u8]) -> Result<Secret, &'static str> {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .ok_or("CloudflareTunnel must be namespaced")?;
    let owner_ref = tunnel
        .controller_owner_ref(&())
        .ok_or("failed to build owner reference")?;

    let mut data = BTreeMap::new();
    data.insert(
        "credentials.json".to_string(),
        ByteString(credentials_json.to_vec()),
    );

    Ok(Secret {
        metadata: ObjectMeta {
            name: Some(format!("{name}-tunnel-credentials")),
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
            "metadata": {
                "name": name,
                "namespace": ns,
                "uid": "test-uid-1234",
            },
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
    fn secret_has_correct_name() {
        let tunnel = test_tunnel("my-tunnel", "default");
        let secret = build(&tunnel, b"creds-json").unwrap();
        assert_eq!(
            secret.metadata.name.unwrap(),
            "my-tunnel-tunnel-credentials"
        );
    }

    #[test]
    fn secret_has_correct_namespace() {
        let tunnel = test_tunnel("my-tunnel", "my-ns");
        let secret = build(&tunnel, b"creds").unwrap();
        assert_eq!(secret.metadata.namespace.unwrap(), "my-ns");
    }

    #[test]
    fn secret_has_managed_by_label() {
        let tunnel = test_tunnel("t", "default");
        let secret = build(&tunnel, b"c").unwrap();
        let labels = secret.metadata.labels.unwrap();
        assert_eq!(
            labels["app.kubernetes.io/managed-by"],
            "cloudflare-tunnel-operator"
        );
    }

    #[test]
    fn secret_has_owner_reference() {
        let tunnel = test_tunnel("t", "default");
        let secret = build(&tunnel, b"c").unwrap();
        let orefs = secret.metadata.owner_references.unwrap();
        assert_eq!(orefs.len(), 1);
        assert_eq!(orefs[0].name, "t");
    }

    #[test]
    fn secret_contains_credentials_data() {
        let tunnel = test_tunnel("t", "default");
        let creds = b"test-credentials-json";
        let secret = build(&tunnel, creds).unwrap();
        let data = secret.data.unwrap();
        assert_eq!(data["credentials.json"].0, creds);
    }
}
