use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use thiserror::Error;

use crate::{Database, ErrorEmission, RepeatedErrorAggregator, UsageRequestRecord};

#[derive(Debug, Error)]
pub enum UsageQueueError {
    #[error("usage writer is unavailable")]
    Unavailable,
    #[error("usage writer flush timed out")]
    FlushTimeout,
}

#[derive(Clone)]
pub struct UsageWriter {
    inner: Arc<UsageWriterInner>,
}

struct UsageWriterInner {
    sender: mpsc::Sender<UsageWriterMessage>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    dropped_records: Arc<AtomicU64>,
}

enum UsageWriterMessage {
    Record(Box<UsageRequestRecord>),
    Flush(mpsc::SyncSender<()>),
    Shutdown,
}

impl UsageWriter {
    pub fn start(database: Arc<Database>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let dropped_records = Arc::new(AtomicU64::new(0));
        let worker_dropped_records = dropped_records.clone();
        let worker = thread::Builder::new()
            .name("hal100-usage-writer".to_owned())
            .spawn(move || run_usage_writer(database, receiver, worker_dropped_records))
            .expect("HAL100 must be able to start the Usage writer");
        Self {
            inner: Arc::new(UsageWriterInner {
                sender,
                worker: Mutex::new(Some(worker)),
                dropped_records,
            }),
        }
    }

    pub fn record(&self, usage: UsageRequestRecord) -> Result<(), UsageQueueError> {
        let result = self
            .inner
            .sender
            .send(UsageWriterMessage::Record(Box::new(usage)))
            .map_err(|_| UsageQueueError::Unavailable);
        if result.is_err() {
            self.inner.dropped_records.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    pub fn dropped_record_count(&self) -> u64 {
        self.inner.dropped_records.load(Ordering::Relaxed)
    }

    pub fn flush(&self, timeout: Duration) -> Result<(), UsageQueueError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.inner
            .sender
            .send(UsageWriterMessage::Flush(sender))
            .map_err(|_| UsageQueueError::Unavailable)?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| UsageQueueError::FlushTimeout)
    }
}

impl Drop for UsageWriterInner {
    fn drop(&mut self) {
        let _ = self.sender.send(UsageWriterMessage::Shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = worker.join();
        }
    }
}

fn run_usage_writer(
    database: Arc<Database>,
    receiver: mpsc::Receiver<UsageWriterMessage>,
    dropped_records: Arc<AtomicU64>,
) {
    let errors = RepeatedErrorAggregator::new(Duration::from_secs(60));
    while let Ok(message) = receiver.recv() {
        match message {
            UsageWriterMessage::Record(usage) => {
                if let Err(error) = database.insert_usage_request(&usage) {
                    dropped_records.fetch_add(1, Ordering::Relaxed);
                    match errors.observe("usage_persist_failed") {
                        ErrorEmission::Emit => tracing::error!(
                            error_code = "usage_persist_failed",
                            error = %error,
                            "usage_persist_failed"
                        ),
                        ErrorEmission::Suppress => {}
                        ErrorEmission::EmitWithSummary { suppressed } => tracing::error!(
                            error_code = "usage_persist_failed",
                            suppressed,
                            error = %error,
                            "usage_persist_failed_repeated"
                        ),
                    }
                }
            }
            UsageWriterMessage::Flush(completed) => {
                let _ = completed.send(());
            }
            UsageWriterMessage::Shutdown => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_each_completed_request_once_on_a_dedicated_worker() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let writer = UsageWriter::start(database.clone());
        writer.record(sample_usage()).expect("queue usage");
        writer
            .flush(Duration::from_secs(1))
            .expect("flush usage writer");

        assert_eq!(database.usage_request_count().expect("usage count"), 1);
    }

    fn sample_usage() -> UsageRequestRecord {
        UsageRequestRecord {
            request_id: "request-1".to_owned(),
            client_app_id: "client-1".to_owned(),
            protocol: "openai_chat_completions".to_owned(),
            requested_model: "test-model".to_owned(),
            resolved_model: "test-model".to_owned(),
            backend_id: "backend-1".to_owned(),
            started_at_ms: 1,
            first_token_at_ms: Some(2),
            completed_at_ms: 3,
            input_tokens: Some(4),
            cached_tokens: Some(1),
            output_tokens: Some(5),
            total_tokens: Some(9),
            status: "succeeded".to_owned(),
            error_category: None,
            usage_accuracy: "exact_backend_response".to_owned(),
        }
    }
}
