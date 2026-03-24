# Architecture

## Overview

The cloudflare-tunnel-operator is a Kubernetes controller written in Rust using the [kube-rs](https://kube.rs) framework. It watches `CloudflareTunnel` custom resources and reconciles the desired state by creating and managing resources both inside the cluster and in the Cloudflare API.

## Traffic Flow

```
Internet
  │
  ▼
Cloudflare Edge  (terminates TLS, applies WAF/DDoS rules)
  │  CNAME: <hostname> → <tunnel-id>.cfargotunnel.com
  ▼
cloudflared pod  (outbound-only connection to CF edge)
  │  runs inside cluster, no inbound ports required
  ▼
Gateway  (gateway.networking.k8s.io/v1 Gateway)
  │  managed by GatewayClass (e.g., Cilium)
  ▼
HTTPRoute  (created by the application team)
  │  matches host + path, selects backend
  ▼
Service / Pod
```

The key property is that `cloudflared` opens an outbound connection to Cloudflare's edge. No inbound firewall rules or load balancer IP addresses are needed.

## Reconcile Loop

The controller uses a finalizer-aware reconcile loop. On every reconcile the following steps run in order:

1. **Resolve zone** — Call `GET /zones?name=<zone>` to obtain the Cloudflare zone ID and account ID.

2. **Ensure tunnel** — Check `status.tunnelId`. If it is set, verify the tunnel still exists via `GET /accounts/<id>/cfd_tunnel/<tid>`. If not set (or the tunnel was deleted externally), call `POST /accounts/<id>/cfd_tunnel` to create a new one and store the returned credentials JSON.

3. **Sync DNS** — For each hostname in `spec.gateway.listeners`, call `ensure_dns_cname`, which idempotently creates a proxied CNAME record pointing to `<tunnel-id>.cfargotunnel.com`. After creating/verifying the desired records, any CNAME records previously created by the operator (identified by the comment `Managed by cloudflare-tunnel-operator`) that are no longer in the spec are deleted.

4. **Sync Secret** — If step 2 produced new credentials, server-side apply a Secret named `<cr-name>-tunnel-credentials` containing the credentials JSON. This Secret is mounted into the `cloudflared` pod.

5. **Sync ConfigMap** — Server-side apply a ConfigMap named `<cr-name>-config` containing the `cloudflared` configuration file that references the credentials Secret and sets the tunnel ID.

6. **Sync Deployment** — Server-side apply a Deployment named `<cr-name>-cloudflared` that runs the `cloudflared` image with the config and credentials volumes mounted.

7. **Sync Gateway** — Server-side apply a Gateway resource (`gateway.networking.k8s.io/v1`) named `<cr-name>-gateway`. Each listener in the spec becomes a Gateway listener on port 80 with `allowedRoutes.namespaces.from: All`, so `HTTPRoute` resources in any namespace can bind to it.

8. **Update status** — Patch `status.tunnelId`, `status.conditions`, and `status.routes` with the current state.

On success the controller requeues after 5 minutes (300 s) so it can detect and correct any drift. On error it requeues after 15 seconds.

## Resource Ownership

All cluster resources created by the operator (Secret, ConfigMap, Deployment, Gateway) have an `ownerReference` pointing to the `CloudflareTunnel` CR. This means:

- Kubernetes garbage-collects them automatically when the CR is deleted (after the finalizer runs).
- The controller uses `Controller::owns(...)` so changes to owned resources trigger an immediate reconcile.

## Finalizer Cleanup

The operator registers the finalizer `tunnels.abutt.dev/cleanup` on every `CloudflareTunnel`. When a deletion is requested the finalizer runs before the object is removed:

1. Look up the Cloudflare zone.
2. Delete all CNAME records with the comment `Managed by cloudflare-tunnel-operator` in that zone.
3. Delete the Cloudflare tunnel itself (first clearing active connections, then deleting the tunnel record).
4. Remove the finalizer, allowing Kubernetes to delete the object.

If the zone cannot be resolved (e.g., the token has been revoked), the cleanup logs a warning and skips the Cloudflare API cleanup to avoid blocking CR deletion indefinitely.

## Cloudflare API Interactions

| Operation                 | Cloudflare API endpoint                                                           |
| ------------------------- | --------------------------------------------------------------------------------- |
| Resolve zone              | `GET /client/v4/zones?name=<zone>`                                                |
| Create tunnel             | `POST /client/v4/accounts/<id>/cfd_tunnel`                                        |
| Get tunnel                | `GET /client/v4/accounts/<id>/cfd_tunnel/<tid>`                                   |
| Delete tunnel connections | `DELETE /client/v4/accounts/<id>/cfd_tunnel/<tid>/connections`                    |
| Delete tunnel             | `DELETE /client/v4/accounts/<id>/cfd_tunnel/<tid>`                                |
| List DNS records          | `GET /client/v4/zones/<id>/dns_records?type=CNAME&comment.contains=Managed+by...` |
| Create DNS record         | `POST /client/v4/zones/<id>/dns_records`                                          |
| Delete DNS record         | `DELETE /client/v4/zones/<id>/dns_records/<id>`                                   |

All DNS records are created as proxied CNAMEs (orange-cloud). The comment field (`Managed by cloudflare-tunnel-operator`) is used to distinguish records managed by the operator from records created manually.

## Gateway API Integration

The operator creates a `gateway.networking.k8s.io/v1 Gateway` object. The Gateway spec sets:

- `gatewayClassName` — from `spec.gateway.gatewayClassName`
- One listener per entry in `spec.gateway.listeners`, each named `listener-N` with protocol `HTTP` on port 80 and `allowedRoutes.namespaces.from: All`

The GatewayClass controller (e.g., Cilium) is responsible for provisioning the actual proxy and making the Gateway `Accepted`. The operator does not configure TLS termination on the Gateway — TLS is handled by Cloudflare at the edge.

Application teams create `HTTPRoute` resources that reference the managed Gateway as a `parentRef`. Routes can live in any namespace because the listeners use `allowedRoutes.namespaces.from: All`.

## Controller Configuration

The operator reads configuration from environment variables:

| Variable       | Description                                                                           |
| -------------- | ------------------------------------------------------------------------------------- |
| `CF_API_TOKEN` | Cloudflare API token used as the default for all CRs that do not set `credentialsRef` |
| `RUST_LOG`     | Log filter (e.g., `cloudflare_tunnel_operator=info,warn`)                             |

The operator uses leader election via a Kubernetes Lease in the `cloudflare-operator` namespace to support running multiple replicas safely (only one replica reconciles at a time).
