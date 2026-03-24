# Troubleshooting

## Checking Controller Logs

All operator logs are structured JSON. The easiest way to follow them:

```bash
kubectl logs -n cloudflare-operator deployment/cloudflare-tunnel-operator -f
```

Filter for errors only:

```bash
kubectl logs -n cloudflare-operator deployment/cloudflare-tunnel-operator \
  | jq 'select(.level == "ERROR" or .level == "WARN")'
```

Check events on a specific tunnel:

```bash
kubectl describe cft <tunnel-name> -n <namespace>
```

---

## Common Issues

### CRD Not Installed

**Symptom:** `kubectl apply` of a `CloudflareTunnel` resource fails with:

```
error: resource mapping not found for name: "..." namespace: "..." from "...":
no matches for kind "CloudflareTunnel" in version "tunnels.abutt.dev/v1alpha1"
```

**Fix:** Apply the CRD before applying any `CloudflareTunnel` resources:

```bash
kubectl apply -f deploy/crd.yaml
```

Verify the CRD is established:

```bash
kubectl get crd cloudflaretunnels.tunnels.abutt.dev
```

---

### API Token Permission Error

**Symptom:** Controller logs show:

```json
{ "level": "ERROR", "message": "Cloudflare error: API error: ..." }
```

or the tunnel's `Ready` condition is `False` with a reason mentioning authorization or permissions.

**Fix:** Ensure the token has all three required permissions:

- `Zone > DNS > Edit`
- `Account > Cloudflare Tunnel > Edit`
- `Zone > Zone > Read`

Verify the Secret exists and has the correct key:

```bash
kubectl get secret cloudflare-api-token -n cloudflare-operator -o jsonpath='{.data.token}' | base64 -d | wc -c
# Should print a non-zero length
```

Test the token directly:

```bash
curl -s -H "Authorization: Bearer <TOKEN>" \
  "https://api.cloudflare.com/client/v4/user/tokens/verify" | jq .
```

---

### Tunnel Stuck as "Inactive" in Cloudflare Dashboard

**Symptom:** The Cloudflare dashboard shows the tunnel as inactive or the `cloudflared` pod is not connecting.

**Diagnosis:** Check the `cloudflared` pod logs:

```bash
kubectl logs -n <namespace> deployment/<tunnel-name>-cloudflared
```

Common causes:

1. **Credentials Secret missing or empty** — The Secret `<tunnel-name>-tunnel-credentials` must exist and contain valid JSON. Check:

   ```bash
   kubectl get secret <tunnel-name>-tunnel-credentials -n <namespace>
   kubectl describe secret <tunnel-name>-tunnel-credentials -n <namespace>
   ```

2. **ConfigMap missing** — The ConfigMap `<tunnel-name>-config` must exist:

   ```bash
   kubectl get configmap <tunnel-name>-config -n <namespace>
   ```

3. **Pod not scheduling** — Check for pending pods:

   ```bash
   kubectl get pods -n <namespace> -l app.kubernetes.io/managed-by=cloudflare-tunnel-operator
   kubectl describe pod -n <namespace> <pod-name>
   ```

4. **Image pull failure** — If using a custom image, ensure the image is accessible from the cluster.

---

### DNS Records Not Created

**Symptom:** No CNAME records appear in the Cloudflare DNS dashboard after creating a `CloudflareTunnel`.

**Diagnosis:**

1. Check the tunnel's status conditions:

   ```bash
   kubectl get cft <tunnel-name> -n <namespace> -o jsonpath='{.status.conditions}' | jq .
   ```

2. Look for API errors in the controller logs mentioning `dns_records`.

3. Verify the `zone` field matches the exact zone name in your Cloudflare account (e.g., `example.com`, not `www.example.com`).

4. Confirm the API token has `Zone > DNS > Edit` permission for the target zone.

5. Check whether the record already exists with a different content — the operator will delete and recreate it only if the CNAME target does not match.

---

### Gateway Not Routing Traffic

**Symptom:** DNS resolves correctly and `cloudflared` is connected, but HTTP requests return errors or are not reaching the backend.

**Diagnosis:**

1. Check that the Gateway exists and is `Accepted`:

   ```bash
   kubectl get gateway <tunnel-name>-gateway -n <namespace>
   kubectl describe gateway <tunnel-name>-gateway -n <namespace>
   ```

   The GatewayClass controller (e.g., Cilium) must accept the Gateway before it routes traffic.

2. Verify the `gatewayClassName` in the `CloudflareTunnel` spec matches an existing GatewayClass:

   ```bash
   kubectl get gatewayclass
   ```

3. Check that the `HTTPRoute` references the correct Gateway name and namespace:

   ```yaml
   spec:
     parentRefs:
       - name: <tunnel-name>-gateway
         namespace: <namespace>
   ```

4. Check the `HTTPRoute` status for route acceptance:

   ```bash
   kubectl describe httproute <route-name> -n <namespace>
   ```

5. Check that the Gateway API CRDs are installed:

   ```bash
   kubectl get crd gateways.gateway.networking.k8s.io
   ```

---

### CloudflareTunnel Stuck Deleting

**Symptom:** `kubectl delete cft` hangs and the resource remains with a `DeletionTimestamp` set.

**Cause:** The finalizer `tunnels.abutt.dev/cleanup` is running and blocked, usually because the Cloudflare API call is failing.

**Diagnosis:**

```bash
kubectl logs -n cloudflare-operator deployment/cloudflare-tunnel-operator | grep cleanup
```

**Options:**

1. Fix the underlying issue (e.g., restore API token permissions) and wait for the cleanup to complete.

2. If the Cloudflare resources have already been deleted manually and you want to force-remove the CR, remove the finalizer directly (this skips Cloudflare cleanup):

   ```bash
   kubectl patch cft <tunnel-name> -n <namespace> \
     --type json \
     -p '[{"op":"remove","path":"/metadata/finalizers"}]'
   ```

---

### Multiple Tunnels Interfere with Each Other

**Symptom:** DNS records from one `CloudflareTunnel` are unexpectedly deleted when another tunnel is reconciled.

**Cause:** Both tunnels share the same zone and the stale-record cleanup uses the comment `Managed by cloudflare-tunnel-operator` to find records to delete. A record that belongs to one tunnel will be deleted if it is not in the other tunnel's listener list.

**Fix:** Each `CloudflareTunnel` should own a disjoint set of hostnames. Do not create two tunnels in the same zone with overlapping listener hostnames.
