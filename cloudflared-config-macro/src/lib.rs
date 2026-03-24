mod parser;

use proc_macro::TokenStream;
use std::path::PathBuf;

/// Reads a vendored Go file and generates Rust structs with serde attributes.
///
/// Usage:
/// ```ignore
/// cloudflared_config_types!("codegen/configuration.go");
/// ```
#[proc_macro]
pub fn cloudflared_config_types(input: TokenStream) -> TokenStream {
    let lit: syn::LitStr = syn::parse(input).expect("expected a string literal path");
    let relative_path = lit.value();

    // Resolve relative to the calling crate's manifest directory
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let file_path: PathBuf = [&manifest_dir, &relative_path].iter().collect();

    let source = std::fs::read_to_string(&file_path).unwrap_or_else(|e| {
        panic!(
            "failed to read Go source file at {}: {}",
            file_path.display(),
            e
        )
    });

    let tokens = parser::parse_go_structs(&source);
    tokens.into()
}
