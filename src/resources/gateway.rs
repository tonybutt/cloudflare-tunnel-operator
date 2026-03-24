use std::collections::BTreeMap;

use kube::api::ApiResource;
use kube::api::DynamicObject;
use kube::{Resource, ResourceExt};
use serde_json::json;

use crate::crd::CloudflareTunnel;

/// ApiResource definition for gateway.networking.k8s.io/v1 Gateway.
pub const GATEWAY_AR: ApiResource = ApiResource {
    group: String::new(),
    version: String::new(),
    api_version: String::new(),
    kind: String::new(),
    plural: String::new(),
};

/// Initialise the Gateway ApiResource at runtime since const String isn't possible.
pub fn gateway_api_resource() -> ApiResource {
    ApiResource {
        group: "gateway.networking.k8s.io".to_string(),
        version: "v1".to_string(),
        api_version: "gateway.networking.k8s.io/v1".to_string(),
        kind: "Gateway".to_string(),
        plural: "gateways".to_string(),
    }
}

/// Builds a Gateway as a DynamicObject for the Gateway API.
///
/// Each listener gets a name like `listener-0`, port 80, protocol HTTP,
/// with allowedRoutes from All namespaces.
pub fn build(tunnel: &CloudflareTunnel) -> Result<DynamicObject, &'static str> {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .ok_or("CloudflareTunnel must be namespaced")?;
    let owner_ref = tunnel
        .controller_owner_ref(&())
        .ok_or("failed to build owner reference")?;

    let listeners: Vec<serde_json::Value> = tunnel
        .spec
        .gateway
        .listeners
        .iter()
        .enumerate()
        .map(|(i, l)| {
            json!({
                "name": format!("listener-{i}"),
                "hostname": l.hostname,
                "port": 80,
                "protocol": "HTTP",
                "allowedRoutes": {
                    "namespaces": {
                        "from": "All"
                    }
                }
            })
        })
        .collect();

    let ar = gateway_api_resource();

    let mut obj = DynamicObject::new(&name, &ar).within(&namespace);
    obj.metadata.name = Some(format!("{name}-gateway"));
    obj.metadata.owner_references = Some(vec![owner_ref]);
    obj.metadata.labels = Some(managed_by_labels());
    obj.data = json!({
        "spec": {
            "gatewayClassName": tunnel.spec.gateway.gateway_class_name,
            "listeners": listeners
        }
    });

    Ok(obj)
}

fn managed_by_labels() -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "cloudflare-tunnel-operator".to_string(),
    );
    labels
}
