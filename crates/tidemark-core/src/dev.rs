//! Owned by: W0-B1
//! Dev contracts and helpers.

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// DevKind contract.
pub enum DevKind {
    /// Disk variant.
    Disk,
    /// Partition variant.
    Partition,
    /// Md variant.
    Md {
        #[doc = "RAID level reported by sysfs."]
        level: String,
    },
    /// Dm variant.
    Dm,
    /// Other variant.
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// BlockDevInfo contract.
pub struct BlockDevInfo {
    #[doc = "name field; see the normative plan for units."]
    pub name: String,
    #[doc = "major field; see the normative plan for units."]
    pub major: u32,
    #[doc = "minor field; see the normative plan for units."]
    pub minor: u32,
    #[doc = "kind field; see the normative plan for units."]
    pub kind: DevKind,
    #[doc = "rotational field; see the normative plan for units."]
    pub rotational: Option<bool>,
    #[doc = "scheduler field; see the normative plan for units."]
    pub scheduler: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
/// DiskStat contract.
pub struct DiskStat {
    #[doc = "read_ios field; see the normative plan for units."]
    pub read_ios: u64,
    #[doc = "read_merges field; see the normative plan for units."]
    pub read_merges: u64,
    #[doc = "read_sectors field; see the normative plan for units."]
    pub read_sectors: u64,
    #[doc = "read_ticks_ms field; see the normative plan for units."]
    pub read_ticks_ms: u64,
    #[doc = "write_ios field; see the normative plan for units."]
    pub write_ios: u64,
    #[doc = "write_merges field; see the normative plan for units."]
    pub write_merges: u64,
    #[doc = "write_sectors field; see the normative plan for units."]
    pub write_sectors: u64,
    #[doc = "write_ticks_ms field; see the normative plan for units."]
    pub write_ticks_ms: u64,
    #[doc = "in_flight field; see the normative plan for units."]
    pub in_flight: u64,
    #[doc = "io_ticks_ms field; see the normative plan for units."]
    pub io_ticks_ms: u64,
    #[doc = "time_in_queue_ms field; see the normative plan for units."]
    pub time_in_queue_ms: u64,
}
