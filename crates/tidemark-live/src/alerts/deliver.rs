//! Owned by: W5-D
//! Alert delivery interface stubs.
use tidemark_core::live::AlertEvent;
/// Reserved for W5-D.
pub trait Deliver: Send {
    /// Reserved for W5-D.
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError>;
}
/// Reserved for W5-D.
pub struct JournalDeliver;
/// Reserved for W5-D.
pub struct WebhookDeliver;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum DeliverError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}

impl Deliver for JournalDeliver {
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError> {
        let _ = ev;
        Err(DeliverError::NotYetImplemented)
    }
}
impl Deliver for WebhookDeliver {
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError> {
        let _ = ev;
        Err(DeliverError::NotYetImplemented)
    }
}
