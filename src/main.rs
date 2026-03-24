use kube::CustomResourceExt;

mod cloudflare;
mod crd;

fn main() {
    // Print CRD YAML for generation
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("crd") {
        print!(
            "{}",
            serde_json::to_string_pretty(&crd::CloudflareTunnel::crd()).unwrap()
        );
        return;
    }

    println!("cloudflare-tunnel-operator");
}
