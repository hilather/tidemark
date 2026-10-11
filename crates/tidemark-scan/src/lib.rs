//! Owned by: W0-A
//! tidemark-scan crate skeleton.

#![forbid(unsafe_code)]

/// agg module skeleton.
pub mod agg;
/// agg/lca module skeleton.
#[path = "agg/lca.rs"]
mod agg_lca;
/// agg/linkset module skeleton.
#[path = "agg/linkset.rs"]
mod agg_linkset;
/// agg/notable module skeleton.
#[path = "agg/notable.rs"]
mod agg_notable;
/// agg/rollup module skeleton.
#[path = "agg/rollup.rs"]
mod agg_rollup;
/// bulk module skeleton.
pub mod bulk;
/// bulk/join module skeleton.
#[path = "bulk/join.rs"]
mod bulk_join;
/// bulk/names module skeleton.
#[path = "bulk/names.rs"]
mod bulk_names;
/// mode module skeleton.
pub mod mode;
/// orchestrator module skeleton.
pub mod orchestrator;
/// progress module skeleton.
pub mod progress;
