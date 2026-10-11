//! Owned by: W5-D
//! Structured journal events and bounded asynchronous webhook delivery.
use parking_lot::{Condvar, Mutex};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tidemark_core::live::AlertEvent;
/// Alert delivery sink. Webhook delivery enqueues without waiting for network I/O.
pub trait Deliver: Send {
    /// Submit an event to this sink.
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError>;
}
/// Emit structured tracing fields for the daemon's journald subscriber.
pub struct JournalDeliver;
/// Bounded webhook worker; dropping it stops retries and discards queued events.
pub struct WebhookDeliver {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    host: String,
}
struct Shared {
    queue: Mutex<(VecDeque<serde_json::Value>, bool)>,
    wake: Condvar,
    dropped: AtomicU64,
    failed: AtomicU64,
}
/// Delivery construction or shutdown failure.
#[derive(Debug, thiserror::Error)]
pub enum DeliverError {
    /// Worker construction failed.
    #[error("delivery failed: {0}")]
    Failed(String),
    /// Sink no longer accepts events.
    #[error("delivery stopped")]
    Stopped,
    /// Legacy unavailable implementation error.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
impl Deliver for JournalDeliver {
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError> {
        tracing::info!(TIDEMARK_RULE = %ev.rule, TIDEMARK_SUBJECT = %ev.subject, TIDEMARK_STATE = ?ev.state, TIDEMARK_VALUE = ?ev.value, message = %ev.message);
        Ok(())
    }
}
impl WebhookDeliver {
    /// Start a worker with a 1,000-event queue and a five-second request timeout.
    pub fn new(url: &str, host: &str) -> Result<Self, DeliverError> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(DeliverError::Failed("expected HTTP(S) URL".into()));
        }
        let shared = Arc::new(Shared {
            queue: Mutex::new((VecDeque::new(), false)),
            wake: Condvar::new(),
            dropped: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        });
        let state = shared.clone();
        let url = url.to_owned();
        let worker = thread::Builder::new()
            .name("tidemark-webhook".into())
            .spawn(move || {
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(5)))
                    .proxy(None)
                    .max_redirects(0)
                    .build()
                    .into();
                loop {
                    let body = {
                        let mut q = state.queue.lock();
                        while q.0.is_empty() && !q.1 {
                            state.wake.wait(&mut q);
                        }
                        if q.1 {
                            break;
                        }
                        q.0.pop_front()
                    };
                    let Some(body) = body else {
                        continue;
                    };
                    for attempt in 0..4 {
                        if agent
                            .post(&url)
                            .send_json(&body)
                            .is_ok_and(|r| r.status().is_success())
                        {
                            break;
                        }
                        if attempt == 3 {
                            state.failed.fetch_add(1, Ordering::Relaxed);
                            tracing::warn!("webhook exhausted retries");
                            break;
                        }
                        let mut q = state.queue.lock();
                        if q.1 {
                            return;
                        }
                        let delay = Duration::from_secs(1 << attempt);
                        let start = std::time::Instant::now();
                        while !q.1 && start.elapsed() < delay {
                            state
                                .wake
                                .wait_for(&mut q, delay.saturating_sub(start.elapsed()));
                        }
                        if q.1 {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| DeliverError::Failed(e.to_string()))?;
        Ok(Self {
            shared,
            worker: Some(worker),
            host: host.into(),
        })
    }
    /// Number of oldest queued events discarded at the queue cap.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }
    /// Number of events whose initial request and three retries failed.
    pub fn failed(&self) -> u64 {
        self.shared.failed.load(Ordering::Relaxed)
    }
}
impl Deliver for WebhookDeliver {
    fn deliver(&mut self, ev: &AlertEvent) -> Result<(), DeliverError> {
        let subject = &ev.subject;
        let message = &ev.message;
        let body = serde_json::json!({"rule":ev.rule,"subject":subject,"state":ev.state,"value":ev.value,"message":message,"host":self.host,"ts":ev.ts,"text":format!("{} {}: {}",ev.rule,subject,message)});
        let mut q = self.shared.queue.lock();
        if q.1 {
            return Err(DeliverError::Stopped);
        }
        if q.0.len() == 1000 {
            q.0.pop_front();
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
        }
        q.0.push_back(body);
        self.shared.wake.notify_one();
        Ok(())
    }
}
impl Drop for WebhookDeliver {
    fn drop(&mut self) {
        {
            let mut q = self.shared.queue.lock();
            q.1 = true;
            q.0.clear();
            self.shared.wake.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidemark_core::live::AlertState;
    fn event() -> AlertEvent {
        AlertEvent {
            ts: 42,
            rule: "full".into(),
            subject: "/secret/path".into(),
            state: AlertState::Firing,
            value: Some(95.0),
            message: "/secret/path full".into(),
        }
    }
    #[test]
    fn webhook_json_preserves_fields() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let mut sink =
            WebhookDeliver::new(&format!("http://{}", server.server_addr()), "host").unwrap();
        sink.deliver(&event()).unwrap();
        let mut request = server
            .recv_timeout(Duration::from_secs(6))
            .unwrap()
            .unwrap();
        assert_eq!(request.method(), &tiny_http::Method::Post);
        let mut body = String::new();
        request.as_reader().read_to_string(&mut body).unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["subject"], "/secret/path");
        assert_eq!(value["rule"], "full");
        assert_eq!(value["message"], "/secret/path full");
        assert_eq!(value["state"], "firing");
        assert_eq!(value["value"], 95.0);
        assert_eq!(value["host"], "host");
        assert_eq!(value["ts"], 42);
        assert!(body.contains("secret"));
        assert_eq!(value["text"], "full /secret/path: /secret/path full");
        request.respond(tiny_http::Response::empty(200)).unwrap();
    }
    #[test]
    fn retries_are_bounded() {
        retry_status(503);
    }
    #[test]
    fn redirects_are_failed_deliveries() {
        retry_status(302);
    }
    fn retry_status(status: u16) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let mut sink =
            WebhookDeliver::new(&format!("http://{}", server.server_addr()), "host").unwrap();
        sink.deliver(&event()).unwrap();
        for _ in 0..4 {
            let request = server
                .recv_timeout(Duration::from_secs(6))
                .unwrap()
                .unwrap();
            assert_eq!(request.url(), "/");
            let mut response = tiny_http::Response::empty(status);
            if status == 302 {
                response
                    .add_header(tiny_http::Header::from_bytes("Location", "/redirected").unwrap());
            }
            request.respond(response).unwrap();
        }
        for _ in 0..100 {
            if sink.failed() == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sink.failed(), 1);
        assert!(server
            .recv_timeout(Duration::from_millis(200))
            .unwrap()
            .is_none());
    }
    #[test]
    fn queue_drops_oldest() {
        let shared = Arc::new(Shared {
            queue: Mutex::new((VecDeque::new(), false)),
            wake: Condvar::new(),
            dropped: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        });
        let mut sink = WebhookDeliver {
            shared,
            worker: None,
            host: "host".into(),
        };
        for ts in 0..1001 {
            let mut ev = event();
            ev.ts = ts;
            sink.deliver(&ev).unwrap();
        }
        assert_eq!(sink.dropped(), 1);
        assert_eq!(sink.shared.queue.lock().0.front().unwrap()["ts"], 1);
        sink.shared.queue.lock().1 = true;
        assert!(matches!(sink.deliver(&event()), Err(DeliverError::Stopped)));
        assert!(WebhookDeliver::new("file:///bad", "host").is_err());
        JournalDeliver.deliver(&event()).unwrap();
    }
}

#[cfg(test)]
mod timeout_test {
    use super::*;
    #[test]
    fn request_timeout_bounds_shutdown() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let mut sink =
            WebhookDeliver::new(&format!("http://{}", server.server_addr()), "host").unwrap();
        let event = AlertEvent {
            ts: 0,
            rule: "test".into(),
            subject: "state".into(),
            state: tidemark_core::live::AlertState::Firing,
            value: None,
            message: "test".into(),
        };
        sink.deliver(&event).unwrap();
        let request = server
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        let start = std::time::Instant::now();
        drop(sink);
        assert!(start.elapsed() < Duration::from_secs(7));
        drop(request);
    }
}
