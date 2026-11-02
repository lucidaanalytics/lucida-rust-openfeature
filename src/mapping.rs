//! Lucida <-> OpenFeature mappings: reason table (`testdata/openfeature/reasons.json`), flag metadata and the
//! evaluation-context split.

use lucida::{FlagDetails, FlagOptions, FlagValue};
use open_feature::{
    EvaluationContext, EvaluationContextFieldValue, EvaluationReason, FlagMetadata, StructValue,
    Value as OfValue,
};
use serde_json::{Map, Value, json};
use time::format_description::well_known::Rfc3339;

/// OpenFeature reason of a raw Lucida reason; a variant string under `condition_match` is `Split`.
#[must_use]
pub fn reason(raw: &str, value: &FlagValue) -> EvaluationReason {
    match raw {
        "condition_match" if matches!(value, FlagValue::Variant(_)) => EvaluationReason::Split,
        "condition_match" => EvaluationReason::TargetingMatch,
        "sticky_bucket" => EvaluationReason::Cached,
        "no_condition_match" | "out_of_rollout" | "holdout" | "namespace_excluded"
        | "no_bucketing_key" | "prerequisite_failed" | "prerequisite_missing" | "fallback" => {
            EvaluationReason::Default
        }
        "prerequisite_cycle" => EvaluationReason::Error,
        "inactive" | "kill_switch" | "app_context_excluded" => EvaluationReason::Disabled,
        "preview" | "user_preview" => EvaluationReason::Static,
        _ => EvaluationReason::Unknown,
    }
}

/// `lucidaReason`, `conditionIndex` (when >= 0), `requestId` (when set), `evaluatedLocally`.
#[must_use]
pub fn flag_metadata(d: &FlagDetails) -> FlagMetadata {
    let mut m = FlagMetadata::default().with_value("lucidaReason", d.reason.clone());
    if let Ok(i) = i32::try_from(d.condition_index)
        && i >= 0
    {
        m = m.with_value("conditionIndex", i);
    }
    if let Some(r) = d.request_id.clone() {
        m = m.with_value("requestId", r);
    }
    m.with_value("evaluatedLocally", d.locally_evaluated)
}

/// Targeting key plus Lucida inputs: `groups` (type -> key) and `groupProperties` (type -> map) are their own
/// attributes, every other attribute is a person property. Struct attributes must hold a `serde_json::Value` or a
/// `StructValue` (anything else is dropped).
#[must_use]
pub fn inputs(ctx: &EvaluationContext) -> (Option<String>, FlagOptions) {
    let mut o = FlagOptions::default();
    for (k, v) in &ctx.custom_fields {
        let Some(j) = field_json(v) else { continue };
        match (k.as_str(), j) {
            ("groups", Value::Object(m)) => {
                o.groups = m
                    .into_iter()
                    .map(|(t, key)| (t, Value::String(key.as_str().map_or_else(|| key.to_string(), str::to_owned))))
                    .collect();
            }
            ("groupProperties", Value::Object(m)) => o.group_properties = m,
            (_, j) => {
                o.person_properties.insert(k.clone(), j);
            }
        }
    }
    (ctx.targeting_key.clone(), o)
}

fn field_json(v: &EvaluationContextFieldValue) -> Option<Value> {
    Some(match v {
        EvaluationContextFieldValue::Bool(b) => json!(*b),
        EvaluationContextFieldValue::Int(i) => json!(*i),
        EvaluationContextFieldValue::Float(f) => json!(*f),
        EvaluationContextFieldValue::String(s) => json!(s),
        EvaluationContextFieldValue::DateTime(t) => json!(t.format(&Rfc3339).ok()?),
        EvaluationContextFieldValue::Struct(a) => {
            let any = a.as_ref();
            if let Some(j) = any.downcast_ref::<Value>() {
                j.clone()
            } else {
                struct_json(any.downcast_ref::<StructValue>()?)
            }
        }
    })
}

fn struct_json(s: &StructValue) -> Value {
    Value::Object(s.fields.iter().map(|(k, v)| (k.clone(), of_json(v))).collect())
}

fn of_json(v: &OfValue) -> Value {
    match v {
        OfValue::Bool(b) => json!(*b),
        OfValue::Int(i) => json!(*i),
        OfValue::Float(f) => json!(*f),
        OfValue::String(s) => json!(s),
        OfValue::Array(a) => Value::Array(a.iter().map(of_json).collect()),
        OfValue::Struct(s) => struct_json(s),
    }
}

/// A JSON payload object as an OpenFeature structure (nulls are dropped).
pub(crate) fn to_struct(m: &Map<String, Value>) -> StructValue {
    StructValue {
        fields: m
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), to_of(v)?)))
            .collect(),
    }
}

fn to_of(v: &Value) -> Option<OfValue> {
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => OfValue::Bool(*b),
        Value::Number(n) => n
            .as_i64()
            .map_or_else(|| OfValue::Float(n.as_f64().unwrap_or_default()), OfValue::Int),
        Value::String(s) => OfValue::String(s.clone()),
        Value::Array(a) => OfValue::Array(a.iter().filter_map(to_of).collect()),
        Value::Object(m) => OfValue::Struct(to_struct(m)),
    })
}
