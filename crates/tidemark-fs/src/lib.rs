//! Owned by: W0-A
//! tidemark-fs crate skeleton.

#![forbid(unsafe_code)]

/// attribution module skeleton.
pub mod attribution;
/// blockdev module skeleton.
pub mod blockdev;
/// canon module skeleton.
pub mod canon;
/// lvm module skeleton.
pub mod lvm;
/// mount module skeleton.
pub mod mount;
/// probe module skeleton.
pub mod probe;
/// procfs module skeleton.
pub mod procfs;
/// symlinks module skeleton.
pub mod symlinks;
/// throttle module skeleton.
pub mod throttle;
/// walk module skeleton.
pub mod walk;
/// walk/getdents module skeleton.
#[path = "walk/getdents.rs"]
mod walk_getdents;
/// walk/worker module skeleton.
#[path = "walk/worker.rs"]
mod walk_worker;
