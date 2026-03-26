# Tunnel Token Auth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Switch from credentials-file auth to token-based auth using the `TUNNEL_TOKEN` env var, fetched via the Cloudflare API.

**Architecture:** Add a `get_tunnel_token` method to `CloudflareApi` that calls `GET /accounts/{account_id}/cfd_tunnel/{tunnel_id}/token`. The secret stores the token string instead of credentials JSON. The deployment injects it as `TUNNEL_TOKEN` env var (no creds volume mount). The config drops `credentials_file` since the token handles auth. The token is fetched on every reconcile (not just on create) so the secret stays current.

**Tech Stack:** Rust, kube-rs, reqwest, serde, k8s-openapi

---

## File Structure

| File                          | Action    | Responsibility                                               |
| ----------------------------- | --------- | ------------------------------------------------------------ |
| `src/cloudflare/client.rs`    | Modify    | Add `get_tunnel_token` to trait + impl                       |
| `src/cloudflare/types.rs`     | No change | Token endpoint returns `CfResponse<String>`, already exists  |
| `src/resources/secret.rs`     | Modify    | Store `token` key instead of `credentials.json`              |
| `src/resources/deployment.rs` | Modify    | Replace creds volume with `TUNNEL_TOKEN` env var from secret |
| `src/resources/configmap.rs`  | Modify    | Stop setting `credentials_file`                              |
| `src/cloudflared_config.rs`   | Modify    | Remove `credentials_file` from `CloudflaredConfigFile`       |
| `src/controller.rs`           | Modify    | Fetch token after ensure_tunnel, always sync secret          |

---

### Task 1: Add `get_tunnel_token` to CloudflareApi

**Files:**

- Modify: `src/cloudflare/client.rs:8-38` (trait), `src/cloudflare/client.rs:80-377` (impl)

- [ ] **Step 1: Add method to trait**

Add to the `CloudflareApi` trait:

```rust
async fn get_tunnel_token(
    &self,
    account_id: &str,
    tunnel_id: &str,
) -> Result<String, CloudflareError>;
```

- [ ] **Step 2: Implement the method**

Add to `impl CloudflareApi for CloudflareClient`:

```rust
async fn get_tunnel_token(
    &self,
    account_id: &str,
    tunnel_id: &str,
) -> Result<String, CloudflareError> {
    let endpoint = format!("{CF_API_BASE}/accounts/{account_id}/cfd_tunnel/{tunnel_id}/token");
    let resp = self
        .http
        .get(&endpoint)
        .header("Authorization", self.auth_header())
        .send()
        .await
        .map_err(|e| CloudflareError::Http {
            source: e,
            endpoint: endpoint.clone(),
        })?;

    let resp = resp.error_for_status().map_err(|e| CloudflareError::Http {
        source: e,
        endpoint: endpoint.clone(),
    })?;

    let body: CfResponse<String> =
        resp.json()
            .await
            .map_err(|e| CloudflareError::Deserialize {
                source: e,
                endpoint,
            })?;

    Ok(body.result)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles (no tests call it yet, but trait + impl must agree)

- [ ] **Step 4: Commit**

```bash
git add src/cloudflare/client.rs
git commit -m "feat(cloudflare): add get_tunnel_token API method"
```

---

### Task 2: Remove `credentials_file` from config

**Files:**

- Modify: `src/cloudflared_config.rs:9-15`
- Modify: `src/resources/configmap.rs:25-32`

- [ ] **Step 1: Remove `credentials_file` field and update tests**

In `src/cloudflared_config.rs`, update `CloudflaredConfigFile` to remove the `credentials_file` field:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct CloudflaredConfigFile {
    pub tunnel: String,
    pub ingress: Vec<UnvalidatedIngressRule>,
}
```

Update the tests in the same file — remove all `credentials_file` references from test structs and assertions. Remove the `configmap_has_credentials_path` test in `src/resources/configmap.rs`.

- [ ] **Step 2: Update configmap builder**

In `src/resources/configmap.rs`, remove the `credentials_file` line from the `CloudflaredConfigFile` construction:

```rust
let config = CloudflaredConfigFile {
    tunnel: tunnel_id.to_string(),
    ingress: vec![UnvalidatedIngressRule {
        service: format!("http://{gateway_svc}"),
        ..Default::default()
    }],
};
```

- [ ] **Step 3: Run tests**

Run: `cargo test`
Expected: all existing tests pass (with updated assertions)

- [ ] **Step 4: Commit**

```bash
git add src/cloudflared_config.rs src/resources/configmap.rs
git commit -m "refactor: remove credentials_file from cloudflared config"
```

---

### Task 3: Switch secret to store tunnel token

**Files:**

- Modify: `src/resources/secret.rs`

- [ ] **Step 1: Update secret builder**

Change the `build` function signature and body. The secret now stores a `token` key with the tunnel token string:

```rust
/// Builds a Secret containing the tunnel token.
///
/// The secret is named `{name}-tunnel-credentials` and stores the token
/// under the key `token`.
pub fn build(tunnel: &CloudflareTunnel, token: &str) -> Result<Secret, &'static str> {
    let name = tunnel.name_any();
    let namespace = tunnel
        .namespace()
        .ok_or("CloudflareTunnel must be namespaced")?;
    let owner_ref = tunnel
        .controller_owner_ref(&())
        .ok_or("failed to build owner reference")?;

    let mut data = BTreeMap::new();
    data.insert(
        "token".to_string(),
        ByteString(token.as_bytes().to_vec()),
    );

    Ok(Secret {
        metadata: ObjectMeta {
            name: Some(format!("{name}-tunnel-credentials")),
            namespace: Some(namespace),
            owner_references: Some(vec![owner_ref]),
            labels: Some(managed_by_labels()),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    })
}
```

- [ ] **Step 2: Update tests**

Update all tests in `secret.rs` to pass a `&str` token instead of `&[u8]` credentials, and assert on the `"token"` key:

- `build(&tunnel, b"creds-json")` → `build(&tunnel, "test-token")`
- `build(&tunnel, b"creds")` → `build(&tunnel, "tok")`
- `build(&tunnel, b"c")` → `build(&tunnel, "t")`
- `secret_contains_credentials_data` test: assert `data["token"].0` equals `b"test-token"`

- [ ] **Step 3: Run tests**

Run: `cargo test --lib secret`
Expected: all secret tests pass

- [ ] **Step 4: Commit**

```bash
git add src/resources/secret.rs
git commit -m "refactor(secret): store tunnel token instead of credentials JSON"
```

---

### Task 4: Switch deployment to use TUNNEL_TOKEN env var

**Files:**

- Modify: `src/resources/deployment.rs`

- [ ] **Step 1: Write new failing test**

Add a test in `src/resources/deployment.rs`:

```rust
#[test]
fn deployment_injects_tunnel_token_env() {
    let tunnel = test_tunnel("web", "default");
    let deploy = build(&tunnel).unwrap();
    let container = &deploy.spec.unwrap().template.spec.unwrap().containers[0];
    let env = container.env.as_ref().expect("env vars should be set");
    let token_env = env.iter().find(|e| e.name == "TUNNEL_TOKEN").expect("TUNNEL_TOKEN env var");
    let source = token_env.value_from.as_ref().expect("should use valueFrom");
    let secret_ref = source.secret_key_ref.as_ref().expect("should ref a secret");
    assert_eq!(secret_ref.key, "token");
    assert!(secret_ref.name.contains("tunnel-credentials"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib deployment::tests::deployment_injects_tunnel_token_env`
Expected: FAIL

- [ ] **Step 3: Update the deployment builder**

Replace the creds volume mount and volume with a `TUNNEL_TOKEN` env var. Update the import block — remove `SecretVolumeSource` (no longer used) and add `EnvVar`, `EnvVarSource`, `SecretKeySelector`:

```rust
use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec,
    SecretKeySelector, Volume, VolumeMount,
};
```

In the container, replace `volume_mounts` and remove the creds volume:

```rust
containers: vec![Container {
    name: "cloudflared".to_string(),
    image: Some(image),
    args: Some(vec![
        "tunnel".to_string(),
        "--config".to_string(),
        "/etc/cloudflared/config/config.yaml".to_string(),
        "run".to_string(),
    ]),
    env: Some(vec![EnvVar {
        name: "TUNNEL_TOKEN".to_string(),
        value_from: Some(EnvVarSource {
            secret_key_ref: Some(SecretKeySelector {
                name: format!("{name}-tunnel-credentials"),
                key: "token".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }]),
    volume_mounts: Some(vec![VolumeMount {
        name: "config".to_string(),
        mount_path: "/etc/cloudflared/config".to_string(),
        read_only: Some(true),
        ..Default::default()
    }]),
    ..Default::default()
}],
volumes: Some(vec![Volume {
    name: "config".to_string(),
    config_map: Some(ConfigMapVolumeSource {
        name: format!("{name}-config"),
        ..Default::default()
    }),
    ..Default::default()
}]),
```

- [ ] **Step 4: Update existing tests**

- `deployment_mounts_creds_and_config`: rename to `deployment_mounts_config_volume`, assert only 1 volume mount (`config`), no `creds` mount.
- `deployment_references_correct_secret`: remove this test (no creds volume anymore).

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib deployment`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add src/resources/deployment.rs
git commit -m "feat(deployment): use TUNNEL_TOKEN env var instead of credentials volume"
```

---

### Task 5: Update controller to fetch and sync token

**Files:**

- Modify: `src/controller.rs:123-163` (apply function), `src/controller.rs:244-280` (ensure_tunnel)

- [ ] **Step 1: Change `ensure_tunnel` return type**

`ensure_tunnel` currently returns `(String, Option<String>)` where the second value is the credentials JSON (only on create). Change it to return just the `tunnel_id` — the token will be fetched separately:

```rust
async fn ensure_tunnel(
    tunnel: &CloudflareTunnel,
    cf: &dyn CloudflareApi,
    account_id: &str,
) -> Result<String, Error> {
```

In the body, remove credential handling:

- `Some(_) => return Ok(tid.clone()),`
- At the end: `Ok(t.id)`

- [ ] **Step 2: Update `create_tunnel` return type**

In `src/cloudflare/client.rs`, change `create_tunnel` to return just `Tunnel` instead of `(Tunnel, String)`:

Trait:

```rust
async fn create_tunnel(
    &self,
    account_id: &str,
    name: &str,
) -> Result<Tunnel, CloudflareError>;
```

Impl: remove the `creds` json construction and return `Ok(body.result)`. Note: `base64` and `rand` are still needed for `tunnel_secret` generation in the create request — do not remove them.

- [ ] **Step 3: Update `apply` to fetch token and always sync secret**

In the `apply` function, replace steps 2 and 4:

```rust
// 2. Ensure tunnel exists
let tunnel_id = ensure_tunnel(&tunnel, cf, &account_id).await?;

// ...step 3 unchanged...

// 4. Fetch tunnel token and sync Secret
let token = cf
    .get_tunnel_token(&account_id, &tunnel_id)
    .await
    .map_err(|e| Error::Cloudflare {
        source: e,
        operation: "get_tunnel_token",
    })?;
let secret =
    resources::secret::build(&tunnel, &token).map_err(Error::InvalidResource)?;
let secret_api: Api<Secret> = Api::namespaced(client.clone(), &ns);
let pp = PatchParams::apply(MANAGER).force();
secret_api
    .patch(
        &format!("{name}-tunnel-credentials"),
        &pp,
        &Patch::Apply(&secret),
    )
    .await
    .map_err(|e| Error::Kube {
        source: e,
        operation: "sync_secret",
    })?;
```

Note: the `if let Some(creds)` guard is removed — the token is fetched every reconcile so the secret always stays current.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: compiles cleanly

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add src/controller.rs src/cloudflare/client.rs
git commit -m "feat: fetch tunnel token via API and sync to secret every reconcile"
```

---

### Task 6: Update documentation

**Files:**

- Modify: `docs/configuration.md`
- Modify: `README.md`

- [ ] **Step 1: Update docs to reflect token-based auth**

In any documentation that mentions `credentials.json` or the credentials secret format, update to describe the `token` key and `TUNNEL_TOKEN` env var approach.

- [ ] **Step 2: Commit**

```bash
git add docs/configuration.md README.md
git commit -m "docs: update auth docs for tunnel token approach"
```
