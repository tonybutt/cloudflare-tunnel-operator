mod tunnel_lifecycle;

use cloudflare_tunnel_operator::cloudflare::client::{CloudflareApi, CloudflareClient};
use cloudflare_tunnel_operator::controller;
use kube::Client;

const TEST_ZONE: &str = "anthonybutt.software";

/// Read CF_API_TOKEN from environment, panic if not set.
pub fn cf_token() -> String {
    std::env::var("CF_API_TOKEN").expect("CF_API_TOKEN must be set for e2e tests")
}

/// Create a kind cluster, install CRDs (operator + Gateway API), and return a kube Client.
pub async fn setup_kind_cluster(name: &str) -> Client {
    // Delete any leftover cluster from a previous failed run
    let _ = tokio::process::Command::new("kind")
        .args(["delete", "cluster", "--name", name])
        .status()
        .await;

    let status = tokio::process::Command::new("kind")
        .args(["create", "cluster", "--name", name, "--wait", "60s"])
        .status()
        .await
        .expect("failed to run kind");
    assert!(status.success(), "kind create cluster failed");

    let output = tokio::process::Command::new("kind")
        .args(["get", "kubeconfig", "--name", name])
        .output()
        .await
        .expect("failed to get kind kubeconfig");
    assert!(output.status.success(), "kind get kubeconfig failed");

    let kubeconfig = String::from_utf8(output.stdout).expect("invalid kubeconfig utf8");

    let config = kube::Config::from_custom_kubeconfig(
        kube::config::Kubeconfig::from_yaml(&kubeconfig).expect("invalid kubeconfig"),
        &kube::config::KubeConfigOptions::default(),
    )
    .await
    .expect("failed to build kube config");

    let client = Client::try_from(config).expect("failed to create kube client");

    // Install Gateway API CRDs
    let gw_status = tokio::process::Command::new("kubectl")
        .args([
            "apply",
            "-f",
            "https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.2.1/standard-install.yaml",
            "--kubeconfig", "/dev/stdin",
        ])
        .env("KUBECONFIG", "")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn kubectl for Gateway API CRDs");

    // Use kind get kubeconfig and pipe it — simpler to just write kubeconfig to a temp approach
    // Actually, let's use the KUBECONFIG env approach via kind export
    drop(gw_status);

    // Install Gateway API CRDs using kind's kubeconfig
    let gw_output = tokio::process::Command::new("sh")
        .args([
            "-c",
            &format!(
                "kind get kubeconfig --name {name} | kubectl apply --kubeconfig /dev/stdin -f https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.2.1/standard-install.yaml"
            ),
        ])
        .output()
        .await
        .expect("failed to install Gateway API CRDs");
    assert!(
        gw_output.status.success(),
        "Gateway API CRD install failed: {}",
        String::from_utf8_lossy(&gw_output.stderr)
    );
    eprintln!("Gateway API CRDs installed");

    // Install the operator CRD
    use kube::CustomResourceExt;
    let crd = cloudflare_tunnel_operator::crd::CloudflareTunnel::crd();
    let crd_api: kube::Api<k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition> =
        kube::Api::all(client.clone());
    let pp = kube::api::PatchParams::apply("e2e-test").force();
    crd_api
        .patch(
            "cloudflaretunnels.tunnels.abutt.dev",
            &pp,
            &kube::api::Patch::Apply(&crd),
        )
        .await
        .expect("failed to install CRD");

    // Wait for CRDs to be established
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    client
}

/// Delete a kind cluster.
pub async fn teardown_kind_cluster(name: &str) {
    let _ = tokio::process::Command::new("kind")
        .args(["delete", "cluster", "--name", name])
        .status()
        .await;
}

/// Start the controller in a background tokio task with the real Cloudflare client.
pub fn start_controller(client: Client, token: &str) -> tokio::task::JoinHandle<()> {
    // Init tracing for test output
    let _ = tracing_subscriber::fmt()
        .with_env_filter("cloudflare_tunnel_operator=debug,kube=info")
        .with_test_writer()
        .try_init();

    let cf = CloudflareClient::new(token.to_string());
    let ctx = controller::Ctx {
        client,
        cf: Box::new(cf),
    };
    tokio::spawn(async move {
        controller::run(ctx).await;
    })
}

/// Deploy a simple nginx Deployment + Service into the given namespace.
pub async fn deploy_nginx(client: &Client, namespace: &str) {
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::Service;
    use kube::Api;
    use kube::api::PatchParams;

    let deploy: Deployment = serde_json::from_value(serde_json::json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": "nginx",
            "namespace": namespace,
            "labels": { "app": "nginx" }
        },
        "spec": {
            "replicas": 1,
            "selector": { "matchLabels": { "app": "nginx" } },
            "template": {
                "metadata": { "labels": { "app": "nginx" } },
                "spec": {
                    "containers": [{
                        "name": "nginx",
                        "image": "nginx:1.27",
                        "ports": [{ "containerPort": 80 }]
                    }]
                }
            }
        }
    }))
    .expect("failed to build nginx Deployment");

    let svc: Service = serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": "nginx",
            "namespace": namespace,
            "labels": { "app": "nginx" }
        },
        "spec": {
            "selector": { "app": "nginx" },
            "ports": [{ "port": 80, "targetPort": 80 }]
        }
    }))
    .expect("failed to build nginx Service");

    let pp = PatchParams::apply("e2e-test").force();

    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
    deploy_api
        .patch("nginx", &pp, &kube::api::Patch::Apply(&deploy))
        .await
        .expect("failed to deploy nginx Deployment");

    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);
    svc_api
        .patch("nginx", &pp, &kube::api::Patch::Apply(&svc))
        .await
        .expect("failed to deploy nginx Service");

    eprintln!("nginx deployed");
}

/// Create the gateway Service that cloudflared expects to connect to.
///
/// In a real cluster, the Gateway API controller would create this Service
/// when the operator creates the Gateway resource. In kind (without a Gateway
/// controller), we create it manually pointing at the nginx pods.
pub async fn create_gateway_service(client: &Client, tunnel_name: &str, namespace: &str) {
    use k8s_openapi::api::core::v1::Service;
    use kube::Api;
    use kube::api::PatchParams;

    let svc_name = format!("{tunnel_name}-gateway");

    let svc: Service = serde_json::from_value(serde_json::json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": svc_name,
            "namespace": namespace
        },
        "spec": {
            "selector": { "app": "nginx" },
            "ports": [{ "port": 80, "targetPort": 80 }]
        }
    }))
    .expect("failed to build gateway Service");

    let pp = PatchParams::apply("e2e-test").force();
    let svc_api: Api<Service> = Api::namespaced(client.clone(), namespace);
    svc_api
        .patch(&svc_name, &pp, &kube::api::Patch::Apply(&svc))
        .await
        .expect("failed to create gateway Service");

    eprintln!("gateway service '{svc_name}' created");
}

/// Clean up any tunnels and DNS records created during a test.
///
/// This is a safety net — the controller's finalizer should handle cleanup,
/// but if the test fails mid-way, this ensures we don't leave resources behind.
pub async fn cleanup_cloudflare(token: &str, tunnel_name_prefix: &str) {
    let cf = CloudflareClient::new(token.to_string());

    // Resolve zone
    let zone_result = cf.get_zone_id(TEST_ZONE).await;
    let (zone_id, account_id) = match zone_result {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("cleanup: failed to resolve zone: {e}");
            return;
        }
    };

    // Delete any DNS records managed by the operator
    match cf.list_dns_records_by_comment(&zone_id).await {
        Ok(records) => {
            for record in records {
                if record.name.contains(tunnel_name_prefix)
                    || record.content.contains("cfargotunnel.com")
                {
                    eprintln!("cleanup: deleting DNS record {}", record.name);
                    let _ = cf.delete_dns_record(&zone_id, &record.id).await;
                }
            }
        }
        Err(e) => eprintln!("cleanup: failed to list DNS records: {e}"),
    }

    // List and delete any tunnels with the test prefix
    // Use the raw HTTP client since CloudflareClient doesn't have a list method
    let http = reqwest::Client::new();
    let resp = http
        .get(format!(
            "https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .query(&[("name", tunnel_name_prefix), ("is_deleted", "false")])
        .send()
        .await;

    if let Ok(resp) = resp {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(tunnels) = body["result"].as_array() {
                for tunnel in tunnels {
                    if let Some(id) = tunnel["id"].as_str() {
                        let name = tunnel["name"].as_str().unwrap_or("unknown");
                        eprintln!("cleanup: deleting tunnel {name} ({id})");
                        let _ = cf.delete_tunnel(&account_id, id).await;
                    }
                }
            }
        }
    }
}
