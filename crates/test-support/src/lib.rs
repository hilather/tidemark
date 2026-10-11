//! Owned by: W0-C
//! Temporary fixtures and independent metadata reference oracles.
#![forbid(unsafe_code)]
pub mod fixture;
pub mod golden;
#[cfg(feature = "privileged")]
pub mod lvm;
pub mod oracle;
pub mod random;
pub mod reference;
#[cfg(feature = "privileged")]
pub mod xfs;
pub use fixture::{materialize, materialize_file, Fixture};
pub use random::{model_tree, random_tree, ModelTree, RandomTreeParams};
