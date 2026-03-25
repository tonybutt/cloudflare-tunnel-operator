# Configuration Reference

## CloudflareTunnel CRD

```
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
```

**Short name:** `cft`
**Scope:** Namespaced
**Group:** `tunnels.abutt.dev`
**Version:** `v1alpha1`

---

## Spec Fields

### `spec.zone` (required)

The Cloudflare zone name that the tunnel's DNS records will be created in.

```yaml
spec:
  zone: example.com
```

### `spec.gateway` (required)

Controls the Gateway API object created by the operator.

#### `spec.gateway.gatewayClassName` (required)

The name of the `GatewayClass` to use. This must match a GatewayClass present in your cluster (e.g., `cilium`, `nginx`, `istio`).

```yaml
spec:
  gateway:
    gatewayClassName: cilium
```

#### `spec.gateway.listeners` (required)

A list of hostnames the tunnel will serve. Each entry results in:

- A proxied CNAME DNS record in Cloudflare.
- A listener on the managed Gateway object.

Wildcards are supported for a single subdomain level (e.g., `*.example.com`).

```yaml
spec:
  gateway:
    listeners:
      - hostname: app.example.com
      - hostname: "*.example.com"
```

### `spec.image` (optional)

Override the `cloudflared` container image. Defaults to `cloudflare/cloudflared:2026.3.0`.

```yaml
spec:
  image: cloudflare/cloudflared:2025.1.0
```

### `spec.credentialsRef` (optional)

Reference to a Kubernetes Secret that contains a `token` key with a Cloudflare API token. If omitted, the operator uses the `CF_API_TOKEN` environment variable from the controller Deployment.

This allows different `CloudflareTunnel` resources to use different Cloudflare accounts or API tokens.

```yaml
spec:
  credentialsRef:
    name: my-cf-token
    namespace: my-app
```

The referenced Secret must have a `token` key:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: my-cf-token
  namespace: my-app
stringData:
  token: <CLOUDFLARE_API_TOKEN>
```

---

## Status Fields

| Field                                    | Type   | Description                                             |
| ---------------------------------------- | ------ | ------------------------------------------------------- |
| `status.tunnelId`                        | string | The UUID assigned to the tunnel by Cloudflare           |
| `status.conditions`                      | list   | Standard Kubernetes conditions                          |
| `status.conditions[].type`               | string | Always `Ready`                                          |
| `status.conditions[].status`             | string | `True` when last reconcile succeeded, `False` otherwise |
| `status.conditions[].reason`             | string | `Reconciled` on success; error name on failure          |
| `status.conditions[].message`            | string | Human-readable detail                                   |
| `status.conditions[].lastTransitionTime` | time   | When the condition last changed                         |
| `status.routes`                          | list   | Per-hostname status entries                             |
| `status.routes[].hostname`               | string | The hostname this entry describes                       |
| `status.routes[].dnsRecord`              | string | Cloudflare DNS record ID                                |
| `status.routes[].status`                 | string | `Active` when the record is in place                    |

---

## Examples

### Single domain

Expose one service at a fixed hostname:

```yaml
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: blog-tunnel
  namespace: blog
spec:
  zone: example.com
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: blog.example.com
```

### Wildcard domain

Catch all subdomains under a zone with a single tunnel:

```yaml
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: wildcard-tunnel
  namespace: platform
spec:
  zone: example.com
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: "*.example.com"
```

### Multiple listeners

Combine an apex-level subdomain and a wildcard in the same tunnel:

```yaml
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: multi-tunnel
  namespace: platform
spec:
  zone: example.com
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: example.com
      - hostname: www.example.com
      - hostname: api.example.com
      - hostname: "*.internal.example.com"
```

### Custom cloudflared image

Pin a specific `cloudflared` version to match a tested release:

```yaml
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: pinned-tunnel
  namespace: my-app
spec:
  zone: example.com
  image: cloudflare/cloudflared:2025.1.0
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: app.example.com
```

### Per-CR credentials override

Use a separate Cloudflare account for a specific tunnel:

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: secondary-cf-token
  namespace: tenant-a
stringData:
  token: <TENANT_A_API_TOKEN>
---
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: tenant-a-tunnel
  namespace: tenant-a
spec:
  zone: tenant-a.com
  credentialsRef:
    name: secondary-cf-token
    namespace: tenant-a
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: "*.tenant-a.com"
```
