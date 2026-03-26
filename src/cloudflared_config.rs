use serde::{Deserialize, Serialize};

// Generate all cloudflared config types from the vendored Go source
cloudflared_config_macro::cloudflared_config_types!("codegen/configuration.go");

/// Wrapper for the cloudflared config file used when writing config.yaml for
/// a tunnel pod. Token-based auth is used, so no credentials-file is needed.
#[derive(Debug, Serialize, Deserialize)]
pub struct CloudflaredConfigFile {
    pub tunnel: String,
    pub ingress: Vec<UnvalidatedIngressRule>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_serializes_to_valid_yaml() {
        let config = CloudflaredConfigFile {
            tunnel: "test-id".to_string(),
            ingress: vec![UnvalidatedIngressRule {
                service: "http://my-gateway.default.svc.cluster.local".to_string(),
                ..Default::default()
            }],
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("tunnel: test-id"));
        assert!(yaml.contains("service: http://my-gateway"));
        // Should NOT contain "hostname" when empty (default)
        assert!(!yaml.contains("hostname"));
    }

    #[test]
    fn ingress_rule_with_hostname() {
        let config = CloudflaredConfigFile {
            tunnel: "test-id".to_string(),
            ingress: vec![
                UnvalidatedIngressRule {
                    hostname: "app.example.com".to_string(),
                    service: "http://app:80".to_string(),
                    ..Default::default()
                },
                UnvalidatedIngressRule {
                    service: "http_status:404".to_string(),
                    ..Default::default()
                },
            ],
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("hostname: app.example.com"));
        assert!(yaml.contains("http_status:404"));
    }

    #[test]
    fn generated_structs_exist_and_default() {
        // Verify that the macro generated all expected types
        let _config = CloudflaredConfig::default();
        let _origin = OriginRequestConfig::default();
        let _warp = WarpRoutingConfig::default();
        let _access = AccessConfig::default();
        let _ip_rule = IngressIPRule::default();
        let _ingress = UnvalidatedIngressRule::default();
    }
}
