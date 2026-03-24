# Getting Started

This guide walks you through creating a Cloudflare API token, deploying the operator, and exposing your first application through a tunnel for `*.example.com`.

## Step 1: Create a Cloudflare API Token

1. Log in to the [Cloudflare dashboard](https://dash.cloudflare.com).
2. Go to **My Profile > API Tokens > Create Token**.
3. Select **Create Custom Token**.
4. Give the token a name (e.g., `cloudflare-tunnel-operator`).
5. Add the following permissions:

   | Permission type | Resource          | Access |
   | --------------- | ----------------- | ------ |
   | Zone            | DNS               | Edit   |
   | Account         | Cloudflare Tunnel | Edit   |
   | Zone            | Zone              | Read   |

6. Under **Zone Resources**, select **Include > All zones** (or restrict to the specific zone you will use).
7. Click **Continue to summary**, then **Create Token**.
8. Copy the token — you will not be able to view it again.

## Step 2: Install Prerequisites

Install the Gateway API standard channel CRDs if they are not already present:

```bash
kubectl apply -f https://github.com/kubernetes-sigs/gateway-api/releases/download/v1.2.0/standard-install.yaml
```

## Step 3: Deploy the Operator

```bash
# Create the namespace and API token secret
kubectl create namespace cloudflare-operator

kubectl create secret generic cloudflare-api-token \
  --namespace cloudflare-operator \
  --from-literal=token=<YOUR_TOKEN>

# Apply all manifests (CRD, RBAC, Deployment)
kubectl apply -f deploy/
```

Wait for the operator to become ready:

```bash
kubectl rollout status deployment/cloudflare-tunnel-operator -n cloudflare-operator
```

## Step 4: Create a Tunnel for \*.example.com

Create a `CloudflareTunnel` resource in the namespace where your application lives:

```yaml
apiVersion: tunnels.abutt.dev/v1alpha1
kind: CloudflareTunnel
metadata:
  name: example-tunnel
  namespace: my-app
spec:
  zone: example.com
  gateway:
    gatewayClassName: cilium
    listeners:
      - hostname: "*.example.com"
```

Apply it:

```bash
kubectl apply -f example-tunnel.yaml
```

The operator will:

1. Resolve the `example.com` zone ID via the Cloudflare API.
2. Create a new Cloudflare tunnel named `example-tunnel`.
3. Create a proxied CNAME DNS record `*.example.com → <tunnel-id>.cfargotunnel.com`.
4. Store the tunnel credentials JSON in a Secret named `example-tunnel-tunnel-credentials` in `my-app`.
5. Create a ConfigMap named `example-tunnel-config` with the `cloudflared` config.
6. Deploy a `cloudflared` pod named `example-tunnel-cloudflared`.
7. Create a Gateway named `example-tunnel-gateway` with a listener for `*.example.com`.

## Step 5: Verify the Tunnel is Ready

```bash
kubectl get cft -n my-app
# NAME             TUNNEL ID                              READY
# example-tunnel   xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx   True
```

Check the per-route status:

```bash
kubectl get cft example-tunnel -n my-app -o jsonpath='{.status.routes}' | jq .
# [
#   {
#     "dnsRecord": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
#     "hostname": "*.example.com",
#     "status": "Active"
#   }
# ]
```

## Step 6: Route Traffic with an HTTPRoute

Create an `HTTPRoute` that binds to the Gateway the operator created:

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: my-app
  namespace: my-app
spec:
  parentRefs:
    - name: example-tunnel-gateway
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

Once the route is accepted, requests to `https://app.example.com` will flow through Cloudflare's edge into the tunnel and be forwarded to `my-app-service:8080`.

## Next Steps

- See [configuration.md](configuration.md) for the full CRD reference and more examples.
- See [architecture.md](architecture.md) to understand how the operator works internally.
- See [troubleshooting.md](troubleshooting.md) if something is not working.
