//! Owned by: W5-D
//! Structured journal delivery for alert events.
use tidemark_core::live::AlertEvent;
/// Alert delivery sink.
pub trait Deliver: Send {
    /// Submit an event to this sink.
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError>;
}
/// Emit structured tracing fields for the daemon's journald subscriber.
pub struct JournalDeliver;
/// Alert delivery failure.
#[derive(Debug, thiserror::Error)]
pub enum DeliverError {
    /// Sink failed to deliver an event.
    #[error("delivery failed: {0}")]
    Failed(String),
}
impl Deliver for JournalDeliver {
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError> {
        tracing::info!(TIDEMARK_RULE = %ev.rule, TIDEMARK_SUBJECT = %ev.subject, TIDEMARK_STATE = ?ev.state, TIDEMARK_VALUE = ?ev.value, message = %ev.message);
        Ok(())
    }
}
