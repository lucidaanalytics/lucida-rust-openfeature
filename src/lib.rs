//! OpenFeature provider for Lucida over [`lucida::AsyncClient`]. See [`LucidaProvider`].

mod mapping;
mod provider;

pub use mapping::{flag_metadata, inputs, reason};
pub use provider::LucidaProvider;
