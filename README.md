# lucida-openfeature (Rust)

OpenFeature provider over the Lucida Rust SDK (`AsyncClient`). Pinned to `open-feature 0.2` (Apache-2.0).

```rust
use std::sync::Arc;
use lucida::{AsyncClient, FlagValue, Options};
use lucida_openfeature::LucidaProvider;
use open_feature::OpenFeature;

let opts = Options { secret_key: Some("lc_sk_...".into()), fallbacks: [("new-nav".into(), FlagValue::Bool(false))].into(), ..Options::default() };
let mut api = OpenFeature::singleton_mut().await;
api.set_provider(LucidaProvider::new(Arc::new(AsyncClient::new("lc_pk_...", opts)))).await;
let client = api.create_named_client("app");
let ctx = open_feature::EvaluationContext::default().with_targeting_key("user-1").with_custom_field("plan", "pro");
let on = client.get_bool_value("new-nav", Some(&ctx), None).await.unwrap_or(false);
```

- Context: `targeting_key` is the distinct id (missing: `TargetingKeyMissing`); struct attributes `groups` (type => key) and
  `groupProperties` (type => map), holding a `serde_json::Value` or `StructValue`, feed Lucida groups; every other
  attribute is a person property (dates as RFC 3339).
- Boolean and string flags read the flag value; integer, float and struct flags read its JSON payload. A wrong type is
  `TypeMismatch`, an unknown flag `FlagNotFound`.
- Reasons map through `testdata/openfeature/reasons.json` (`condition_match` with a variant is `Split`). An
  `Options::fallbacks` value resolves with reason `Default` and no error. Flag metadata: `lucidaReason`, `conditionIndex`,
  `requestId`, `evaluatedLocally`.
- `provider.track("purchase", &ctx, &details)` captures the event for the targeting key (`value` becomes `$value`).
  The `open-feature` crate has no provider events, so none are emitted.

Not a member of the root Cargo workspace (own `[workspace]`). Tests: `cargo test` in this directory.

---

This repository is a read-only mirror of the Lucida monorepo (synced by a script, never edited by hand); releases and provenance attestations are built here by GitHub Actions. Issues are welcome, changes land upstream.
