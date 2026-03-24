use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// A parsed Go struct field.
struct GoField {
    name: String,
    rust_type: TokenStream,
    serde_rename: Option<String>,
    skip_serializing_if: Option<String>,
    serde_default: bool,
}

/// A parsed Go struct.
struct GoStruct {
    name: String,
    fields: Vec<GoField>,
}

/// Parse all struct definitions from Go source code and return generated Rust token streams.
pub fn parse_go_structs(source: &str) -> TokenStream {
    let structs = extract_structs(source);
    let mut output = TokenStream::new();

    for s in &structs {
        output.extend(generate_rust_struct(s));
    }

    output
}

fn extract_structs(source: &str) -> Vec<GoStruct> {
    let mut structs = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Match "type NAME struct {"
        if let Some(name) = parse_struct_header(line) {
            // Skip unexported types (lowercase first letter)
            if name.chars().next().map_or(true, |c| c.is_lowercase()) {
                i += 1;
                continue;
            }

            i += 1;
            let mut fields = Vec::new();

            while i < lines.len() {
                let field_line = lines[i].trim();
                if field_line == "}" {
                    break;
                }
                if field_line.is_empty() || field_line.starts_with("//") {
                    i += 1;
                    continue;
                }

                if let Some(field) = parse_field_line(field_line) {
                    fields.push(field);
                }
                i += 1;
            }

            // Rename Configuration -> CloudflaredConfig
            let rust_name = if name == "Configuration" {
                "CloudflaredConfig".to_string()
            } else {
                name
            };

            structs.push(GoStruct {
                name: rust_name,
                fields,
            });
        }

        i += 1;
    }

    structs
}

fn parse_struct_header(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("type ") {
        return None;
    }
    let rest = &line[5..];
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() >= 2 && parts[1] == "struct" {
        Some(parts[0].to_string())
    } else {
        None
    }
}

fn parse_field_line(line: &str) -> Option<GoField> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("//") {
        return None;
    }

    // Split into: field_name, go_type, and optional tag
    // First, extract the tag (backtick-delimited)
    let (code_part, tag_part) = if let Some(tag_start) = line.find('`') {
        let tag_end = line[tag_start + 1..].find('`').map(|p| tag_start + 1 + p);
        if let Some(tag_end) = tag_end {
            (&line[..tag_start], Some(&line[tag_start + 1..tag_end]))
        } else {
            (line, None)
        }
    } else {
        (line, None)
    };

    let tokens: Vec<&str> = code_part.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }

    let field_name = tokens[0];

    // Skip unexported fields (lowercase first letter)
    if field_name.chars().next().map_or(true, |c| c.is_lowercase()) {
        return None;
    }

    // The Go type is everything between field name and tag (may be multi-token for slices/pointers)
    let go_type = tokens[1..].join(" ");

    // Parse tags
    let yaml_tag = tag_part.and_then(|t| parse_tag_value(t, "yaml"));
    let json_tag = tag_part.and_then(|t| parse_tag_value(t, "json"));

    // Determine serde rename value: prefer yaml tag, then json tag, then snake_case of field name
    let yaml_name = yaml_tag.as_ref().map(|(name, _)| name.clone());
    let json_name = json_tag.as_ref().map(|(name, _)| name.clone());

    let has_omitempty = yaml_tag
        .as_ref()
        .map(|(_, opts)| opts.contains(&"omitempty".to_string()))
        .unwrap_or(false)
        || json_tag
            .as_ref()
            .map(|(_, opts)| opts.contains(&"omitempty".to_string()))
            .unwrap_or(false);

    // Determine the serde rename
    let rename_value = yaml_name
        .filter(|n| !n.is_empty())
        .or(json_name.filter(|n| !n.is_empty()));

    let rust_field_name = go_name_to_snake_case(field_name);

    // Determine if we need an explicit rename
    let serde_rename = rename_value.filter(|v| *v != rust_field_name);

    // Map Go type to Rust type
    let (rust_type, is_option, is_vec, is_bool, is_string) = map_go_type(&go_type);

    // Determine skip_serializing_if
    let skip_serializing_if = if has_omitempty {
        if is_option {
            Some("Option::is_none".to_string())
        } else if is_vec {
            Some("Vec::is_empty".to_string())
        } else if is_string {
            Some("String::is_empty".to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Determine serde default
    let serde_default = if is_vec {
        true
    } else if is_string && has_omitempty {
        true
    } else if is_bool && has_omitempty {
        true
    } else {
        // Struct fields (non-option, non-vec, non-primitive) get default
        false
    };

    Some(GoField {
        name: rust_field_name,
        rust_type,
        serde_rename,
        skip_serializing_if,
        serde_default,
    })
}

fn parse_tag_value(tag: &str, key: &str) -> Option<(String, Vec<String>)> {
    // Tags look like: yaml:"name,omitempty" json:"name,omitempty"
    let pattern = format!("{}:\"", key);
    let start = tag.find(&pattern)?;
    let rest = &tag[start + pattern.len()..];
    let end = rest.find('"')?;
    let value = &rest[..end];

    let parts: Vec<&str> = value.split(',').collect();
    let name = parts[0].to_string();
    let opts: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

    Some((name, opts))
}

/// Returns (rust_type, is_option, is_vec, is_bool, is_string).
fn map_go_type(go_type: &str) -> (TokenStream, bool, bool, bool, bool) {
    let go_type = go_type.trim();

    // Pointer types
    if let Some(inner) = go_type.strip_prefix('*') {
        let inner = inner.trim();
        match inner {
            "string" => (quote! { Option<String> }, true, false, false, false),
            "bool" => (quote! { Option<bool> }, true, false, false, false),
            "int" => (quote! { Option<i64> }, true, false, false, false),
            "uint" => (quote! { Option<u64> }, true, false, false, false),
            "CustomDuration" => (quote! { Option<String> }, true, false, false, false),
            _ => {
                // Pointer to struct type
                let ident = format_ident!("{}", inner);
                (quote! { Option<Box<#ident>> }, true, false, false, false)
            }
        }
    }
    // Slice types
    else if let Some(inner) = go_type.strip_prefix("[]") {
        let inner = inner.trim();
        match inner {
            "string" => (quote! { Vec<String> }, false, true, false, false),
            "int" => (quote! { Vec<i64> }, false, true, false, false),
            "uint" => (quote! { Vec<u64> }, false, true, false, false),
            _ => {
                let ident = format_ident!("{}", inner);
                (quote! { Vec<#ident> }, false, true, false, false)
            }
        }
    }
    // Basic types
    else {
        match go_type {
            "string" => (quote! { String }, false, false, false, true),
            "bool" => (quote! { bool }, false, false, true, false),
            "int" => (quote! { i64 }, false, false, false, false),
            "uint" => (quote! { u64 }, false, false, false, false),
            _ => {
                // Struct type (non-pointer)
                let ident = format_ident!("{}", go_type);
                (quote! { #ident }, false, false, false, false)
            }
        }
    }
}

fn go_name_to_snake_case(name: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = name.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            // Add underscore before uppercase if:
            // - Not the first character
            // - Previous char was lowercase, OR
            // - Next char is lowercase (handles acronyms like "HTTPHost" -> "http_host")
            if i > 0 {
                let prev_upper = chars[i - 1].is_uppercase();
                let next_lower = chars.get(i + 1).map_or(false, |c| c.is_lowercase());

                if !prev_upper || next_lower {
                    result.push('_');
                }
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }

    result
}

fn generate_rust_struct(s: &GoStruct) -> TokenStream {
    let struct_name = format_ident!("{}", s.name);

    let fields: Vec<TokenStream> = s
        .fields
        .iter()
        .map(|f| {
            let field_name = format_ident!("{}", f.name);
            let field_type = &f.rust_type;

            let mut attrs = Vec::new();

            if let Some(ref rename) = f.serde_rename {
                attrs.push(quote! { #[serde(rename = #rename)] });
            }

            if let Some(ref skip) = f.skip_serializing_if {
                attrs.push(quote! { #[serde(skip_serializing_if = #skip)] });
            }

            if f.serde_default {
                attrs.push(quote! { #[serde(default)] });
            }

            quote! {
                #(#attrs)*
                pub #field_name: #field_type,
            }
        })
        .collect();

    quote! {
        #[derive(Debug, Serialize, Deserialize, Default)]
        pub struct #struct_name {
            #(#fields)*
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_case() {
        assert_eq!(go_name_to_snake_case("ConnectTimeout"), "connect_timeout");
        assert_eq!(go_name_to_snake_case("HTTPHostHeader"), "http_host_header");
        assert_eq!(go_name_to_snake_case("NoTLSVerify"), "no_tls_verify");
        assert_eq!(go_name_to_snake_case("TCPKeepAlive"), "tcp_keep_alive");
        assert_eq!(go_name_to_snake_case("Http2Origin"), "http2_origin");
        assert_eq!(go_name_to_snake_case("IPRules"), "ip_rules");
        assert_eq!(go_name_to_snake_case("CAPool"), "ca_pool");
        assert_eq!(go_name_to_snake_case("TunnelID"), "tunnel_id");
        assert_eq!(go_name_to_snake_case("MatchSNIToHost"), "match_sni_to_host");
    }

    #[test]
    fn test_parse_struct_header() {
        assert_eq!(
            parse_struct_header("type Configuration struct {"),
            Some("Configuration".to_string())
        );
        assert_eq!(
            parse_struct_header("type foo struct {"),
            Some("foo".to_string())
        );
        assert_eq!(parse_struct_header("// comment"), None);
    }

    #[test]
    fn test_parse_tag_value() {
        let tag = r#"yaml:"connectTimeout" json:"connectTimeout,omitempty""#;
        let (name, opts) = parse_tag_value(tag, "yaml").unwrap();
        assert_eq!(name, "connectTimeout");
        assert!(opts.is_empty());

        let (name, opts) = parse_tag_value(tag, "json").unwrap();
        assert_eq!(name, "connectTimeout");
        assert_eq!(opts, vec!["omitempty"]);
    }

    #[test]
    fn test_extracts_all_structs() {
        let source = r#"
type UnvalidatedIngressRule struct {
    Hostname      string              `json:"hostname,omitempty"`
    Service       string              `json:"service,omitempty"`
}

type Configuration struct {
    TunnelID      string `yaml:"tunnel"`
    Ingress       []UnvalidatedIngressRule
    sourceFile    string
}
"#;
        let structs = extract_structs(source);
        assert_eq!(structs.len(), 2);
        assert_eq!(structs[0].name, "UnvalidatedIngressRule");
        assert_eq!(structs[0].fields.len(), 2);
        assert_eq!(structs[1].name, "CloudflaredConfig");
        // sourceFile is unexported, should be skipped
        assert_eq!(structs[1].fields.len(), 2);
    }
}
