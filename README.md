# cloudflare-tunnel-operator

A Kubernetes operator that manages Cloudflare Tunnels as native cluster resources. Declare a `CloudflareTunnel` custom resource and the operator automatically creates the Cloudflare tunnel, provisions CNAME DNS records, stores credentials in a Kubernetes Secret, and deploys a `cloudflared` pod — all tied together with a Gateway API `Gateway` object so that `HTTPRoute` resources in any namespace can route traffic through the tunnel without exposing any ports to the internet.

## Prerequisites

- Kubernetes 1.27+ with the [Gateway API CRDs](https://gateway-api.sigs.k8s.io/guides/#install-standard-channel) installed
- A Cloudflare account with at least one zone
- A Cloudflare API token with the following permissions:
  - `Zone > DNS > Edit`
  - `Account > Cloudflare Tunnel > Edit`
  - `Zone > Zone > Read`

## Installation

### Quick install

Apply the all-in-one manifest (namespace, CRD, RBAC, and controller deployment):

```bash
kubectl apply -f https://raw.githubusercontent.com/tonybutt/cloudflare-tunnel-operator/main/deploy/manifests/install.yaml
```

### Kustomize

Reference the deploy directory as a remote base:

```yaml
# kustomization.yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization

resources:
  - https://github.com/tonybutt/cloudflare-tunnel-operator//deploy?ref=main
```

Or apply directly:

```bash
kubectl apply -k https://github.com/tonybutt/cloudflare-tunnel-operator/deploy
```

### Flux CD

Create a `GitRepository` source and a `Kustomization` that points at the `deploy/` directory:

```yaml
apiVersion: source.toolkit.fluxcd.io/v1
kind: GitRepository
metadata:
  name: cloudflare-tunnel-operator
  namespace: flux-system
spec:
  interval: 1h
  url: https://github.com/tonybutt/cloudflare-tunnel-operator
  ref:
    branch: main
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: cloudflare-tunnel-operator
  namespace: flux-system
spec:
  interval: 1h
  sourceRef:
    kind: GitRepository
    name: cloudflare-tunnel-operator
  path: ./deploy
  prune: true
```

### Manual

Apply the individual manifests from the `deploy/` directory:

```bash
kubectl apply -f deploy/namespace.yaml
kubectl apply -f deploy/crd.yaml
kubectl apply -f deploy/rbac.yaml
kubectl apply -f deploy/deployment.yaml
```

### Create the API token Secret

Whichever method you used above, create the API token secret in the operator namespace:

```bash
kubectl create secret generic cloudflare-api-token \
  --namespace cloudflare-operator \
  --from-literal=token=<YOUR_CLOUDFLARE_API_TOKEN>
```

### 4. Create a CloudflareTunnel resource

```yaml
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: my-tunnel
  namespace: my-app
spec:
  zone: example.com
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: app.example.com
      - hostname: "*.example.com"
```

```bash
kubectl apply -f my-tunnel.yaml
```

The operator will create the Cloudflare tunnel, CNAME records, a `cloudflared` Deployment, and a Gateway in the `my-app` namespace.

### 5. Create an HTTPRoute in the app namespace

Once the tunnel is ready, point traffic to a Service using a standard Gateway API `HTTPRoute`:

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: my-app
  namespace: my-app
spec:
  parentRefs:
    - name: my-tunnel-gateway
      namespace: my-app
  hostnames:
    - app.example.com
  rules:
    - matches:
        - path:
            type: PathPrefix
            value: /
      backendRefs:
        - name: my-app-service
          port: 8080
```

## Configuration Reference

| Field                               | Type   | Required | Default                           | Description                                                              |
| ----------------------------------- | ------ | -------- | --------------------------------- | ------------------------------------------------------------------------ |
| `spec.zone`                         | string | yes      | —                                 | Cloudflare zone name (e.g. `example.com`) used for DNS record management |
| `spec.gateway.gatewayClassName`     | string | yes      | —                                 | Name of the GatewayClass to use (e.g. `cilium`)                          |
| `spec.gateway.listeners`            | list   | yes      | —                                 | One or more hostnames the tunnel will serve                              |
| `spec.gateway.listeners[].hostname` | string | yes      | —                                 | Exact hostname or wildcard (e.g. `app.example.com` or `*.example.com`)   |
| `spec.image`                        | string | no       | `cloudflare/cloudflared:2026.3.0` | Override the `cloudflared` container image                               |
| `spec.credentialsRef.name`          | string | no       | —                                 | Name of a Secret containing a `token` key with a Cloudflare API token    |
| `spec.credentialsRef.namespace`     | string | no       | —                                 | Namespace of the override Secret; required when `credentialsRef` is set  |

When `credentialsRef` is omitted the operator falls back to the `CF_API_TOKEN` environment variable set on the controller Deployment (the `cloudflare-api-token` Secret in `cloudflare-operator`).

## Status Fields

```bash
kubectl get cft -A
# NAME        TUNNEL ID                              READY
# my-tunnel   xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx   True
```

| Field                        | Description                                                                    |
| ---------------------------- | ------------------------------------------------------------------------------ |
| `status.tunnelId`            | The UUID of the Cloudflare tunnel                                              |
| `status.conditions`          | Standard Kubernetes conditions. `Ready=True` when the last reconcile succeeded |
| `status.conditions[].reason` | `Reconciled` on success; an error string on failure                            |
| `status.routes`              | Per-hostname status including the DNS record ID and an `Active` status string  |

## Uninstall

Delete all `CloudflareTunnel` resources first so the finalizer can clean up the Cloudflare tunnel and DNS records:

```bash
kubectl delete cloudflaretunnels --all --all-namespaces
```

Wait until the resources are fully removed (finalizer cleanup runs against the Cloudflare API), then remove the operator and CRD:

```bash
kubectl delete -f deploy/
kubectl delete -f deploy/crd.yaml
```

## Development

The project uses Nix for a reproducible development environment.

```bash
# Enter the dev shell
nix develop

# Build the binary
nix develop -c cargo build

# Run unit tests
nix develop -c cargo test

# Run e2e tests (requires kind in PATH)
nix develop -c cargo test --test e2e

# Regenerate deploy/crd.yaml + install.yaml from Rust types
./scripts/generate.sh
```

See [docs/architecture.md](docs/architecture.md) for an overview of how the operator works internally.
