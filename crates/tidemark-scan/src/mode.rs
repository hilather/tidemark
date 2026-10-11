//! Owned by: W2-C
//! Cross-task interface stubs.

use tidemark_core::scan::ScanMode;
/// Reserved for W2-C.
pub enum ModeRequest {
    /// Contract variant.
    Auto,
    /// Contract variant.
    Walk,
    /// Contract variant.
    Bulk,
}
/// Reserved for W2-C.
pub struct ModeFacts {
    /// Contract field.
    pub is_xfs: bool,
    /// Contract field.
    pub bulkstat_v5: bool,
    /// Contract field.
    pub cap_sys_admin: bool,
    /// Contract field.
    pub ftype: Option<bool>,
    /// Contract field.
    pub root_is_mountpoint: bool,
    /// Contract field.
    pub prev_root_inodes: Option<u64>,
    /// Contract field.
    pub fs_used_inodes: u64,
    /// Contract field.
    pub dirs_estimate: u64,
}
/// Reserved for W2-C.
pub fn choose_mode(f: &ModeFacts, req: &ModeRequest, memory_budget: u64) -> (ScanMode, String) {
    let _ = f;
    let _ = req;
    let _ = memory_budget;
    (ScanMode::Walk, String::new())
}
