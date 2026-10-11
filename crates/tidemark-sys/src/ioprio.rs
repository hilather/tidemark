//! Owned by: W1-D
//! Per-thread Linux I/O scheduling priority.
#![allow(unsafe_code)]

use std::io;

const IOPRIO_WHO_PROCESS: libc::c_int = 1;
const IOPRIO_CLASS_BE: u16 = 2;
const IOPRIO_CLASS_IDLE: u16 = 3;

fn encode(class: u16, level: u8) -> libc::c_int {
    libc::c_int::from((class << 13) | u16::from(level))
}

fn set(class: u16, level: u8) -> io::Result<()> {
    // SAFETY: ioprio_set takes only integer arguments, no pointers. With who=PROCESS
    // and which=0 Linux changes only the calling thread's I/O priority.
    let result = unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0 as libc::c_int,
            encode(class, level),
        )
    };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Set the calling thread to Linux's idle I/O class.
pub fn set_current_thread_idle() -> io::Result<()> {
    set(IOPRIO_CLASS_IDLE, 0)
}

/// Set best-effort I/O priority (0 is highest, 7 lowest).
pub fn set_current_thread_best_effort(level: u8) -> io::Result<()> {
    if level > 7 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "priority must be 0..=7",
        ));
    }
    set(IOPRIO_CLASS_BE, level)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    #[test]
    fn linux_priority_encoding() {
        assert_eq!(encode(IOPRIO_CLASS_IDLE, 0), 3 << 13);
        for level in 0..=7 {
            assert_eq!(encode(IOPRIO_CLASS_BE, level), (2 << 13) | i32::from(level));
        }
        assert_eq!(
            set_current_thread_best_effort(8).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[cfg(feature = "privileged")]
    #[test]
    fn idle_class_as_root() {
        // A separate thread confines the changed priority to this test.
        std::thread::spawn(|| {
            set_current_thread_idle().unwrap();
            // ionice calls ioprio_get; use the worker TID, not the process leader.
            let tid = std::fs::read_link("/proc/thread-self").unwrap();
            let tid = tid.file_name().unwrap();
            let output = std::process::Command::new("ionice")
                .arg("-p")
                .arg(tid)
                .output()
                .unwrap();
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).starts_with("idle"));
        })
        .join()
        .unwrap();
    }
}
