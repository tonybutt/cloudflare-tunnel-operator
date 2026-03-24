use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::api::{Api, DynamicObject, Patch, PatchParams};
use kube::runtime::controller::Action;
use kube::runtime::finalizer::{Event as Finalizer, finalizer};
use kube::{Client, ResourceExt};

use crate::cloudflare::client::CloudflareClient;
use crate::crd::{CloudflareTunnel, CloudflareTunnelStatus, RouteStatus};
use crate::resources;

const FINALIZER: &str = "tunnels.abutt.dev/cleanup";
const MANAGER: &str = "cloudflare-tunnel-operator";
const REQUEUE_INTERVAL: Duration = Duration::from_secs(300);
const ERROR_REQUEUE: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kubernetes error: {0}")]
    Kube(#[source] kube::Error),
    #[error("Cloudflare error: {0}")]
    Cloudflare(#[from] crate::cloudflare::client::CloudflareError),
    #[error("Finalizer error: {0}")]
    Finalizer(#[source] Box<kube::runtime::finalizer::Error<Error>>),
    #[error("Missing field: {0}")]
    MissingField(&'static str),
}

impl From<kube::Error> for Error {
    fn from(e: kube::Error) -> Self {
        Error::Kube(e)
    }
}

pub struct Ctx {
    pub client: Client,
    pub cf: CloudflareClient,
}

pub async fn reconcile(obj: Arc<CloudflareTunnel>, ctx: Arc<Ctx>) -> Result<Action, Error> {
    let ns = obj.namespace().ok_or(Error::MissingField("namespace"))?;
    let api: Api<CloudflareTunnel> = Api::namespaced(ctx.client.clone(), &ns);

    finalizer(&api, FINALIZER, obj, |event| async {
        match event {
            Finalizer::Apply(tunnel) => apply(tunnel, &ctx).await,
            Finalizer::Cleanup(tunnel) => cleanup(tunnel, &ctx).await,
        }
    })
    .await
    .map_err(|e| Error::Finalizer(Box::new(e)))
}

async fn apply(tunnel: Arc<CloudflareTunnel>, ctx: &Ctx) -> Result<Action, Error> {
    let name = tunnel.name_any();
    let ns = tunnel.namespace().ok_or(Error::MissingField("namespace"))?;
    let client = &ctx.client;
    let cf = &ctx.cf;

    // 1. Resolve zone
    let (zone_id, account_id) = cf.get_zone_id(&tunnel.spec.zone).await?;

    // 2. Ensure tunnel exists
    let (tunnel_id, credentials_json) = ensure_tunnel(&tunnel, cf, &account_id).await?;

    // 3. Sync DNS records
    let route_statuses = sync_dns(cf, &zone_id, &tunnel_id, &tunnel).await?;

    // 4. Sync Secret (server-side apply)
    if let Some(creds) = &credentials_json {
        let secret = resources::secret::build(&tunnel, creds.as_bytes());
        let secret_api: Api<Secret> = Api::namespaced(client.clone(), &ns);
        let pp = PatchParams::apply(MANAGER).force();
        secret_api
            .patch(
                &format!("{name}-tunnel-credentials"),
                &pp,
                &Patch::Apply(&secret),
            )
            .await?;
    }

    // 5. Sync ConfigMap
    let configmap = resources::configmap::build(&tunnel, &tunnel_id);
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), &ns);
    let pp = PatchParams::apply(MANAGER).force();
    cm_api
        .patch(&format!("{name}-config"), &pp, &Patch::Apply(&configmap))
        .await?;

    // 6. Sync Deployment
    let deployment = resources::deployment::build(&tunnel);
    let deploy_api: Api<Deployment> = Api::namespaced(client.clone(), &ns);
    let pp = PatchParams::apply(MANAGER).force();
    deploy_api
        .patch(
            &format!("{name}-cloudflared"),
            &pp,
            &Patch::Apply(&deployment),
        )
        .await?;

    // 7. Sync Gateway
    let gateway = resources::gateway::build(&tunnel);
    let gw_ar = resources::gateway::gateway_api_resource();
    let gw_api: Api<DynamicObject> = Api::namespaced_with(client.clone(), &ns, &gw_ar);
    let pp = PatchParams::apply(MANAGER).force();
    gw_api
        .patch(&format!("{name}-gateway"), &pp, &Patch::Apply(&gateway))
        .await?;

    // 8. Update status
    let status = CloudflareTunnelStatus {
        tunnel_id: Some(tunnel_id),
        conditions: vec![Condition {
            last_transition_time: k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                k8s_openapi::jiff::Timestamp::now(),
            ),
            message: "Tunnel reconciled successfully".to_string(),
            observed_generation: tunnel.metadata.generation,
            reason: "Reconciled".to_string(),
            status: "True".to_string(),
            type_: "Ready".to_string(),
        }],
        routes: route_statuses,
    };

    let tunnel_api: Api<CloudflareTunnel> = Api::namespaced(client.clone(), &ns);
    let status_patch = serde_json::json!({
        "apiVersion": "tunnels.abutt.dev/v1alpha1",
        "kind": "CloudflareTunnel",
        "status": status,
    });
    let pp = PatchParams::apply(MANAGER).force();
    tunnel_api
        .patch_status(&name, &pp, &Patch::Apply(&status_patch))
        .await?;

    tracing::info!(tunnel = %name, "reconciled successfully");
    Ok(Action::requeue(REQUEUE_INTERVAL))
}

async fn ensure_tunnel(
    tunnel: &CloudflareTunnel,
    cf: &CloudflareClient,
    account_id: &str,
) -> Result<(String, Option<String>), Error> {
    let name = tunnel.name_any();

    // Check if we already have a tunnel ID in status
    if let Some(ref status) = tunnel.status {
        if let Some(ref tid) = status.tunnel_id {
            // Verify it still exists
            match cf.get_tunnel(account_id, tid).await? {
                Some(_) => return Ok((tid.clone(), None)),
                None => {
                    tracing::warn!(tunnel_id = %tid, "tunnel was deleted externally, recreating");
                }
            }
        }
    }

    // Create a new tunnel
    let (t, creds) = cf.create_tunnel(account_id, &name).await?;
    tracing::info!(tunnel_id = %t.id, "created tunnel");
    Ok((t.id, Some(creds)))
}

async fn sync_dns(
    cf: &CloudflareClient,
    zone_id: &str,
    tunnel_id: &str,
    tunnel: &CloudflareTunnel,
) -> Result<Vec<RouteStatus>, Error> {
    let desired_hostnames: Vec<&str> = tunnel
        .spec
        .gateway
        .listeners
        .iter()
        .map(|l| l.hostname.as_str())
        .collect();

    // Ensure CNAMEs for each listener
    let mut route_statuses = Vec::new();
    for hostname in &desired_hostnames {
        let record = cf.ensure_dns_cname(zone_id, hostname, tunnel_id).await?;
        route_statuses.push(RouteStatus {
            hostname: hostname.to_string(),
            dns_record: record.id,
            status: "Active".to_string(),
        });
    }

    // Remove stale DNS records managed by us but no longer in spec
    let existing = cf.list_dns_records_by_comment(zone_id).await?;
    for record in existing {
        if !desired_hostnames.contains(&record.name.as_str()) {
            tracing::info!(record = %record.name, "removing stale DNS record");
            cf.delete_dns_record(zone_id, &record.id).await?;
        }
    }

    Ok(route_statuses)
}

async fn cleanup(tunnel: Arc<CloudflareTunnel>, ctx: &Ctx) -> Result<Action, Error> {
    let name = tunnel.name_any();
    let cf = &ctx.cf;

    tracing::info!(tunnel = %name, "cleaning up");

    // Resolve zone to get IDs
    let zone_result = cf.get_zone_id(&tunnel.spec.zone).await;
    if let Ok((zone_id, account_id)) = zone_result {
        // Delete DNS records managed by us
        let records = cf.list_dns_records_by_comment(&zone_id).await?;
        for record in records {
            cf.delete_dns_record(&zone_id, &record.id).await?;
        }

        // Delete tunnel if we have its ID
        if let Some(ref status) = tunnel.status {
            if let Some(ref tid) = status.tunnel_id {
                if let Err(e) = cf.delete_tunnel(&account_id, tid).await {
                    tracing::warn!(error = %e, "failed to delete tunnel, it may already be gone");
                }
            }
        }
    } else {
        tracing::warn!("could not resolve zone during cleanup, skipping CF resource cleanup");
    }

    tracing::info!(tunnel = %name, "cleanup complete");
    Ok(Action::await_change())
}

pub fn error_policy(_obj: Arc<CloudflareTunnel>, error: &Error, _ctx: Arc<Ctx>) -> Action {
    tracing::error!(%error, "reconciliation failed");
    Action::requeue(ERROR_REQUEUE)
}

pub async fn run(ctx: Ctx) {
    let client = ctx.client.clone();
    let tunnel_api: Api<CloudflareTunnel> = Api::all(client.clone());
    let deploy_api: Api<Deployment> = Api::all(client.clone());
    let secret_api: Api<Secret> = Api::all(client.clone());
    let cm_api: Api<ConfigMap> = Api::all(client.clone());
    let gw_ar = resources::gateway::gateway_api_resource();
    let gw_api: Api<DynamicObject> = Api::all_with(client.clone(), &gw_ar);

    let ctx = Arc::new(ctx);

    kube::runtime::controller::Controller::new(
        tunnel_api,
        kube::runtime::watcher::Config::default(),
    )
    .owns(deploy_api, kube::runtime::watcher::Config::default())
    .owns(secret_api, kube::runtime::watcher::Config::default())
    .owns(cm_api, kube::runtime::watcher::Config::default())
    .owns_with(gw_api, gw_ar, kube::runtime::watcher::Config::default())
    .shutdown_on_signal()
    .run(reconcile, error_policy, ctx)
    .for_each(|res| async move {
        match res {
            Ok((obj, _)) => tracing::debug!(object = %obj, "reconcile ok"),
            Err(e) => tracing::warn!(error = %e, "reconcile stream error"),
        }
    })
    .await;
}
