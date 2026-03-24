use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::{JsonSchema, json_schema};
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Serialize, Deserialize, Default, Debug, Clone, JsonSchema)]
#[kube(
    group = "tunnels.abutt.dev",
    version = "v1alpha1",
    kind = "CloudflareTunnel",
    plural = "cloudflaretunnels",
    namespaced,
    status = "CloudflareTunnelStatus",
    shortname = "cft",
    printcolumn(name = "Tunnel ID", type_ = "string", json_path = ".status.tunnelId"),
    printcolumn(
        name = "Ready",
        type_ = "string",
        json_path = ".status.conditions[?(@.type=='Ready')].status"
    )
)]
pub struct CloudflareTunnelSpec {
    /// Cloudflare zone name for DNS record management.
    pub zone: String,

    /// Gateway configuration.
    pub gateway: GatewaySpec,

    /// cloudflared container image override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Reference to a Secret containing a Cloudflare API token.
    /// Falls back to the controller-wide default if not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials_ref: Option<SecretRef>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, JsonSchema)]
pub struct GatewaySpec {
    /// GatewayClass to use (e.g., "cilium").
    pub gateway_class_name: String,

    /// Hostnames the tunnel serves. Each gets a DNS record and Gateway listener.
    pub listeners: Vec<Listener>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, JsonSchema)]
pub struct Listener {
    /// Hostname for this listener (e.g., "blog.abutt.dev" or "*.abutt.dev").
    pub hostname: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, JsonSchema)]
pub struct SecretRef {
    /// Name of the Secret.
    pub name: String,

    /// Namespace of the Secret.
    pub namespace: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, JsonSchema)]
pub struct CloudflareTunnelStatus {
    /// The Cloudflare tunnel ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel_id: Option<String>,

    /// Standard Kubernetes conditions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(schema_with = "conditions")]
    pub conditions: Vec<Condition>,

    /// Per-route status.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<RouteStatus>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, JsonSchema)]
pub struct RouteStatus {
    pub hostname: String,
    pub dns_record: String,
    pub status: String,
}

fn conditions(_: &mut schemars::generate::SchemaGenerator) -> schemars::Schema {
    json_schema!({
        "type": "array",
        "x-kubernetes-list-type": "map",
        "x-kubernetes-list-map-keys": ["type"],
        "items": {
            "type": "object",
            "properties": {
                "lastTransitionTime": { "format": "date-time", "type": "string" },
                "message": { "type": "string" },
                "observedGeneration": { "type": "integer", "format": "int64", "default": 0 },
                "reason": { "type": "string" },
                "status": { "type": "string" },
                "type": { "type": "string" }
            },
            "required": ["lastTransitionTime", "message", "reason", "status", "type"],
        },
    })
}
