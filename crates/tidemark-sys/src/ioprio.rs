//! Owned by: W1-D
//! Cross-task signatures reserved for W1-D.

use std::io;
/// Reserved for W1-D.
pub fn set_current_thread_idle() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-D.
pub fn set_current_thread_best_effort(level: u8) -> io::Result<()> {
    let _ = level;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            set_current_thread_idle().err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            set_current_thread_best_effort(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
