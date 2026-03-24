use kube::CustomResourceExt;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

mod cloudflare;
mod controller;
mod crd;
mod resources;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // CRD generation mode
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("crd") {
        print!(
            "{}",
            serde_json::to_string_pretty(&crd::CloudflareTunnel::crd()).unwrap()
        );
        return Ok(());
    }

    // Tracing setup with JSON output and env filter
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with(fmt::layer().json())
        .init();

    tracing::info!("starting cloudflare-tunnel-operator");

    let cf_token =
        std::env::var("CF_API_TOKEN").expect("CF_API_TOKEN environment variable must be set");

    let client = kube::Client::try_default().await?;
    let cf = cloudflare::client::CloudflareClient::new(cf_token);

    let ctx = controller::Ctx { client, cf };
    controller::run(ctx).await;

    Ok(())
}
