//! The provider: Lucida flag reads exposed through `open_feature::provider::FeatureProvider`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lucida::{AsyncClient, Event, FlagDetails, FlagValue};
use open_feature::provider::{FeatureProvider, ProviderMetadata, ResolutionDetails};
use open_feature::{
    EvaluationContext, EvaluationError, EvaluationErrorCode, EvaluationResult, StructValue,
};
use serde_json::{Map, Value};

use crate::mapping::{flag_metadata, inputs, reason, to_struct};

/// OpenFeature provider over [`AsyncClient`]. `targeting_key` is the distinct id; boolean and string flags read the
/// flag value, integer, float and struct flags read its JSON payload. A value from `Options::fallbacks` resolves
/// with reason `Default` and no error. The `open-feature` 0.2 crate has no provider events, so none are emitted.
pub struct LucidaProvider {
    client: Arc<AsyncClient>,
    meta: ProviderMetadata,
}

fn err(code: EvaluationErrorCode, msg: &str) -> EvaluationError {
    EvaluationError::builder().code(code).message(msg).build()
}

fn mismatch(key: &str) -> EvaluationError {
    err(
        EvaluationErrorCode::TypeMismatch,
        &format!("flag {key} does not resolve to the requested type"),
    )
}

fn out<T>(value: T, d: &FlagDetails) -> ResolutionDetails<T> {
    let mut r = ResolutionDetails::new(value);
    r.reason = Some(reason(&d.reason, &d.value));
    r.variant.clone_from(&d.variant);
    r.flag_metadata = Some(flag_metadata(d));
    r
}

impl LucidaProvider {
    #[must_use]
    pub fn new(client: Arc<AsyncClient>) -> Self {
        Self {
            client,
            meta: ProviderMetadata::new("Lucida"),
        }
    }

    /// OpenFeature tracking: event = `name`, distinct id = targeting key, properties = `details` minus `value`, plus
    /// `$value` when `value` is set. Dropped without a targeting key.
    pub fn track(&self, name: &str, ctx: &EvaluationContext, details: &Map<String, Value>) {
        let (Some(id), o) = inputs(ctx) else { return };
        let mut e = Event::new(&id, name);
        for (k, v) in details {
            match (k.as_str(), v) {
                ("value", Value::Null) => {}
                ("value", v) => e = e.property("$value", v.clone()),
                (k, v) => e = e.property(k, v.clone()),
            }
        }
        for (t, key) in &o.groups {
            e = e.group(t, key.as_str().unwrap_or_default());
        }
        self.client.capture(&e);
    }

    async fn details(&self, key: &str, ctx: &EvaluationContext) -> EvaluationResult<FlagDetails> {
        let (id, o) = inputs(ctx);
        let Some(id) = id.filter(|s| !s.is_empty()) else {
            return Err(err(
                EvaluationErrorCode::TargetingKeyMissing,
                "targeting key is required",
            ));
        };
        self.client
            .get_feature_flag_details(key, &id, &o)
            .await
            .ok_or_else(|| {
                err(
                    EvaluationErrorCode::FlagNotFound,
                    &format!("flag {key} not found"),
                )
            })
    }

    fn payload(d: &FlagDetails, key: &str) -> EvaluationResult<Value> {
        d.payload.clone().ok_or_else(|| mismatch(key))
    }
}

#[async_trait]
impl FeatureProvider for LucidaProvider {
    async fn initialize(&mut self, _context: &EvaluationContext) {
        self.client.wait_ready(Duration::from_secs(10)).await;
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.meta
    }

    async fn resolve_bool_value(
        &self,
        flag_key: &str,
        ctx: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<bool>> {
        let d = self.details(flag_key, ctx).await?;
        match d.value {
            FlagValue::Bool(b) => Ok(out(b, &d)),
            FlagValue::Variant(_) => Err(mismatch(flag_key)),
        }
    }

    async fn resolve_string_value(
        &self,
        flag_key: &str,
        ctx: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<String>> {
        let d = self.details(flag_key, ctx).await?;
        match &d.value {
            FlagValue::Variant(s) => Ok(out(s.clone(), &d)),
            FlagValue::Bool(_) => Err(mismatch(flag_key)),
        }
    }

    async fn resolve_int_value(
        &self,
        flag_key: &str,
        ctx: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<i64>> {
        let d = self.details(flag_key, ctx).await?;
        let v = Self::payload(&d, flag_key)?.as_i64();
        v.map(|v| out(v, &d)).ok_or_else(|| mismatch(flag_key))
    }

    async fn resolve_float_value(
        &self,
        flag_key: &str,
        ctx: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<f64>> {
        let d = self.details(flag_key, ctx).await?;
        let v = Self::payload(&d, flag_key)?.as_f64();
        v.map(|v| out(v, &d)).ok_or_else(|| mismatch(flag_key))
    }

    async fn resolve_struct_value(
        &self,
        flag_key: &str,
        ctx: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<StructValue>> {
        let d = self.details(flag_key, ctx).await?;
        match Self::payload(&d, flag_key)? {
            Value::Object(m) => Ok(out(to_struct(&m), &d)),
            _ => Err(mismatch(flag_key)),
        }
    }
}
