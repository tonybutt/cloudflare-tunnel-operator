use serde::Serialize;

/// Top-level cloudflared tunnel configuration.
/// Serializes to the config.yaml format cloudflared expects.
#[derive(Debug, Serialize)]
pub struct CloudflaredConfig {
    pub tunnel: String,
    #[serde(rename = "credentials-file")]
    pub credentials_file: String,
    pub ingress: Vec<IngressRule>,
}

/// A single cloudflared ingress rule.
#[derive(Debug, Serialize)]
pub struct IngressRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub service: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_serializes_to_valid_yaml() {
        let config = CloudflaredConfig {
            tunnel: "test-id".to_string(),
            credentials_file: "/etc/cloudflared/creds/credentials.json".to_string(),
            ingress: vec![IngressRule {
                hostname: None,
                service: "http://my-gateway.default.svc.cluster.local".to_string(),
            }],
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("tunnel: test-id"));
        assert!(yaml.contains("credentials-file:"));
        assert!(yaml.contains("service: http://my-gateway"));
        // Should NOT contain "hostname" when None
        assert!(!yaml.contains("hostname"));
    }

    #[test]
    fn ingress_rule_with_hostname() {
        let config = CloudflaredConfig {
            tunnel: "test-id".to_string(),
            credentials_file: "/creds.json".to_string(),
            ingress: vec![
                IngressRule {
                    hostname: Some("app.example.com".to_string()),
                    service: "http://app:80".to_string(),
                },
                IngressRule {
                    hostname: None,
                    service: "http_status:404".to_string(),
                },
            ],
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("hostname: app.example.com"));
        assert!(yaml.contains("http_status:404"));
    }
}
