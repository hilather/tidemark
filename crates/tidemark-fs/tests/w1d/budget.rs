//! Owned by: WAUDIT-procfs-budget
//! Synthetic inventory admission and allocation-accounting checks.
#![allow(clippy::unwrap_used)]
use super::*;
fn record(len: usize) -> OpenFile {
    OpenFile {
        pid: 1,
        starttime: 1,
        fd: 3,
        target: vec![b'x'; len],
        dev: 1,
        ino: 1,
        nlink: 0,
        size: 1,
        disk: 512,
        flags: None,
    }
}
#[test]
fn long_duplicates_fail_short_targets_fit_and_accounting_is_exact() {
    for (len, count) in [(0, 600), (16, 600), (4000, 600)] {
        let mut inventory = Inventory {
            files: Vec::new(),
            paths: 0,
            scratch: 0,
            limit: 1024 * 1024,
        };
        let mut rejected = false;
        for _ in 0..count {
            if let Err(error) = inventory.push(record(len)) {
                let typed = error
                    .get_ref()
                    .unwrap()
                    .downcast_ref::<InventoryBudgetExceeded>()
                    .unwrap();
                assert_eq!(typed.count, inventory.files.len());
                assert_eq!(typed.bytes, inventory.bytes());
                rejected = true;
                break;
            }
            let allocation = inventory.files.capacity() * std::mem::size_of::<OpenFile>()
                + inventory
                    .files
                    .iter()
                    .map(|f| f.target.capacity())
                    .sum::<usize>();
            assert_eq!(inventory.bytes(), allocation);
            assert!(allocation <= inventory.limit);
        }
        assert_eq!(rejected, len == 4000);
        inventory.truncate(1);
        assert_eq!(inventory.paths, len);
        assert_eq!(
            inventory.bytes(),
            inventory.files.capacity() * std::mem::size_of::<OpenFile>() + len
        );
    }
}
#[test]
fn scratch_and_spare_capacity_are_charged_before_reservation() {
    let mut inventory = Inventory {
        files: Vec::new(),
        paths: 0,
        scratch: 32,
        limit: 256 * std::mem::size_of::<OpenFile>() + 32,
    };
    assert!(inventory.push(record(1)).is_err());
    assert_eq!(inventory.files.capacity(), 0);
    inventory.push(record(0)).unwrap();
    assert_eq!(inventory.bytes(), inventory.limit);
}

#[test]
fn moving_reallocation_is_admitted_with_both_record_buffers() {
    let mut inventory = Inventory {
        files: Vec::new(),
        paths: 0,
        scratch: 0,
        limit: (256 + 512) * std::mem::size_of::<OpenFile>() - 1,
    };
    for _ in 0..256 {
        inventory.push(record(0)).unwrap();
    }
    assert!(inventory.push(record(0)).is_err());
    assert_eq!(inventory.files.len(), 256);
    assert_eq!(inventory.files.capacity(), 256);
}
