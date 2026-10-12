//! Owned by: W6-A2a
//! Scheduler contracts with explicit clocks, zones and temporary scan fixtures.
#![allow(clippy::unwrap_used)]
#[cfg(feature = "privileged")]
#[path = "w6a2/privileged.rs"]
mod privileged;
#[path = "w6a2/runtime.rs"]
mod runtime;
#[path = "w6a2/schedules.rs"]
mod schedules;
