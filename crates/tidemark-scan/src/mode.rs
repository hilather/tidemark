//! Owned by: W2-C
//! Pure scan mode selection with explicit refusal reasons.

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
    if matches!(req, ModeRequest::Walk) {
        return (ScanMode::Walk, "walk explicitly requested".into());
    }
    let unavailable = if !f.is_xfs {
        Some("filesystem is not XFS")
    } else if !f.bulkstat_v5 {
        Some("missing v5 bulkstat support")
    } else if !f.cap_sys_admin {
        Some("missing CAP_SYS_ADMIN")
    } else if f.ftype != Some(true) {
        Some("XFS ftype=1 is required")
    } else {
        None
    };
    if let Some(reason) = unavailable {
        return (ScanMode::Walk, reason.into());
    }
    if matches!(req, ModeRequest::Auto)
        && !f.root_is_mountpoint
        && f.prev_root_inodes
            .is_none_or(|n| u128::from(n) * 2 < u128::from(f.fs_used_inodes))
    {
        return (
            ScanMode::Walk,
            "root covers less than 50% of used inodes or coverage is unknown".into(),
        );
    }
    // Compute here rather than depending on W2-B's currently inert estimate.
    // Rounded-up dents and checked wide arithmetic make overflow conservative.
    let dents = (u128::from(f.fs_used_inodes) * 11).div_ceil(10);
    let estimate = dents * 12 + u128::from(f.dirs_estimate) * 320 + (64u128 << 20);
    if estimate > u128::from(memory_budget) {
        return (
            ScanMode::Walk,
            format!("bulk memory estimate {estimate} exceeds budget {memory_budget}"),
        );
    }
    (
        ScanMode::Bulk,
        if matches!(req, ModeRequest::Bulk) {
            "bulk explicitly requested; v5 bulkstat and resource checks passed".into()
        } else {
            "XFS v5 bulkstat, ftype=1, root coverage and memory checks passed".into()
        },
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn facts() -> ModeFacts {
        ModeFacts {
            is_xfs: true,
            bulkstat_v5: true,
            cap_sys_admin: true,
            ftype: Some(true),
            root_is_mountpoint: false,
            prev_root_inodes: Some(50),
            fs_used_inodes: 100,
            dirs_estimate: 5,
        }
    }
    #[test]
    fn truth_table() {
        for (case, reason) in [
            (0, "not XFS"),
            (1, "v5 bulkstat"),
            (2, "CAP_SYS_ADMIN"),
            (3, "ftype=1"),
            (4, "ftype=1"),
            (5, "50%"),
            (6, "50%"),
            (7, "memory"),
        ] {
            let mut f = facts();
            let mut budget = u64::MAX;
            match case {
                0 => f.is_xfs = false,
                1 => f.bulkstat_v5 = false,
                2 => f.cap_sys_admin = false,
                3 => f.ftype = Some(false),
                4 => f.ftype = None,
                5 => f.prev_root_inodes = Some(49),
                6 => f.prev_root_inodes = None,
                _ => budget = 1,
            }
            let (mode, text) = choose_mode(&f, &ModeRequest::Auto, budget);
            assert_eq!(mode, ScanMode::Walk);
            assert!(text.contains(reason), "{text}");
            assert_eq!(
                choose_mode(&f, &ModeRequest::Walk, budget).0,
                ScanMode::Walk
            );
        }
        assert_eq!(
            choose_mode(&facts(), &ModeRequest::Auto, u64::MAX).0,
            ScanMode::Bulk
        );
        let mut f = facts();
        f.prev_root_inodes = None;
        f.root_is_mountpoint = true;
        assert_eq!(
            choose_mode(&f, &ModeRequest::Auto, u64::MAX).0,
            ScanMode::Bulk
        );
        f.root_is_mountpoint = false;
        assert_eq!(
            choose_mode(&f, &ModeRequest::Bulk, u64::MAX).0,
            ScanMode::Bulk
        );
        f.fs_used_inodes = u64::MAX;
        f.dirs_estimate = u64::MAX;
        assert_eq!(
            choose_mode(&f, &ModeRequest::Bulk, u64::MAX).0,
            ScanMode::Walk
        );
    }
}
