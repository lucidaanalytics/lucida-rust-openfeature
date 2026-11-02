//! Provider tests over an AsyncClient whose definitions come from a pre-filled cache (no network).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lucida::{
    AsyncClient, BeforeSend, FlagDefinitionCache, FlagValue, InMemoryFlagDefinitionCache, Options,
};
use lucida_openfeature::{LucidaProvider, inputs, reason};
use open_feature::provider::FeatureProvider;
use open_feature::{
    EvaluationContext, EvaluationContextFieldValue, EvaluationErrorCode, EvaluationReason,
};
use serde_json::{Value, json};

fn flag(key: &str, active: bool) -> Value {
    json!({"key": key, "type": "boolean", "active": active, "salt": "s", "aggregation": {"kind": "person"},
           "conditions": [{"rollout_pct": 100}]})
}

type Seen = Arc<Mutex<Vec<Value>>>;

async fn provider() -> (LucidaProvider, Seen) {
    let cache = Arc::new(InMemoryFlagDefinitionCache::new(Duration::from_secs(3600)));
    cache.on_flag_definitions_received(&[flag("on", true), flag("off", false)]);
    let seen: Seen = Arc::default();
    let s = Arc::clone(&seen);
    let client = AsyncClient::new(
        "lc_pk_x",
        Options {
            host: "http://127.0.0.1:1".into(), // unknown flags try the server once and fail
            max_retries: 0,
            secret_key: Some("lc_sk_x".into()),
            flag_stream: false,
            flag_poll_interval: Duration::from_secs(3600),
            flush_interval: Duration::from_secs(3600),
            flag_definition_cache: Some(cache),
            fallbacks: [("ghost".to_string(), FlagValue::Bool(true))].into(),
            before_send: Some(BeforeSend::new(move |v| {
                s.lock().expect("lock").push(v);
                None
            })),
            ..Options::default()
        },
    );
    let mut p = LucidaProvider::new(Arc::new(client));
    p.initialize(&EvaluationContext::default()).await;
    (p, seen)
}

fn ctx() -> EvaluationContext {
    EvaluationContext::default()
        .with_targeting_key("user-1")
        .with_custom_field("plan", "pro")
}

fn expected(name: &str) -> EvaluationReason {
    match name {
        "TARGETING_MATCH" => EvaluationReason::TargetingMatch,
        "SPLIT" => EvaluationReason::Split,
        "CACHED" => EvaluationReason::Cached,
        "DEFAULT" => EvaluationReason::Default,
        "DISABLED" => EvaluationReason::Disabled,
        "STATIC" => EvaluationReason::Static,
        "ERROR" => EvaluationReason::Error,
        _ => EvaluationReason::Unknown,
    }
}

#[test]
fn reason_table_matches_the_shared_json() {
    let dir = std::env::var("LUCIDA_TESTDATA")
        .unwrap_or_else(|_| format!("{}/../../testdata", env!("CARGO_MANIFEST_DIR")));
    let raw =
        std::fs::read_to_string(format!("{dir}/openfeature/reasons.json")).expect("reasons.json");
    let table: Value = serde_json::from_str(&raw).expect("json");
    for (k, row) in table["reasons"].as_object().expect("reasons") {
        let plain = row["reason"].as_str().expect("reason");
        let variant = row["variant_reason"].as_str().unwrap_or(plain);
        assert_eq!(reason(k, &FlagValue::Bool(true)), expected(plain), "{k}");
        assert_eq!(
            reason(k, &FlagValue::Variant("v".into())),
            expected(variant),
            "{k}"
        );
    }
}

#[tokio::test]
async fn served_flag_carries_reason_and_metadata() {
    let (p, _) = provider().await;
    let r = p.resolve_bool_value("on", &ctx()).await.expect("on");
    assert!(r.value);
    assert_eq!(r.reason, Some(EvaluationReason::TargetingMatch));
    assert!(r.flag_metadata.is_some());
    let off = p.resolve_bool_value("off", &ctx()).await.expect("off");
    assert_eq!(off.reason, Some(EvaluationReason::Disabled));
}

#[tokio::test]
async fn fallback_keeps_the_value_without_error_and_emits_no_exposure() {
    let (p, seen) = provider().await;
    let r = p
        .resolve_bool_value("ghost", &ctx())
        .await
        .expect("fallback");
    assert!(r.value);
    assert_eq!(r.reason, Some(EvaluationReason::Default));
    assert!(seen.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn errors() {
    let (p, _) = provider().await;
    let e = p
        .resolve_bool_value("nope", &ctx())
        .await
        .expect_err("missing");
    assert_eq!(e.code, EvaluationErrorCode::FlagNotFound);
    let e = p
        .resolve_string_value("on", &ctx())
        .await
        .expect_err("mismatch");
    assert_eq!(e.code, EvaluationErrorCode::TypeMismatch);
    let e = p
        .resolve_bool_value("on", &EvaluationContext::default())
        .await
        .expect_err("no key");
    assert_eq!(e.code, EvaluationErrorCode::TargetingKeyMissing);
}

#[test]
fn context_split() {
    let c = ctx()
        .with_custom_field(
            "groups",
            EvaluationContextFieldValue::new_struct(json!({"company": "acme"})),
        )
        .with_custom_field(
            "groupProperties",
            EvaluationContextFieldValue::new_struct(json!({"company": {"seats": 5}})),
        );
    let (id, o) = inputs(&c);
    assert_eq!(id.as_deref(), Some("user-1"));
    assert_eq!(o.person_properties.get("plan"), Some(&json!("pro")));
    assert_eq!(o.groups.get("company"), Some(&json!("acme")));
    assert_eq!(
        o.group_properties.get("company"),
        Some(&json!({"seats": 5}))
    );
    assert!(o.person_properties.get("groups").is_none());
}

#[tokio::test]
async fn track_captures_with_value() {
    let (p, seen) = provider().await;
    let d = json!({"value": 12.5, "plan": "pro"});
    p.track("purchase", &ctx(), d.as_object().expect("obj"));
    let seen = seen.lock().expect("lock");
    assert_eq!(seen[0]["event"], "purchase");
    assert_eq!(seen[0]["distinct_id"], "user-1");
    assert_eq!(seen[0]["properties"]["$value"], 12.5);
    assert!(seen[0]["properties"].get("value").is_none());
}
