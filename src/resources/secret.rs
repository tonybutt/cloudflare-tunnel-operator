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
