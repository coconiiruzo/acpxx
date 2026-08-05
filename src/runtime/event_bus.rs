use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::{DiagnosticEvent, DiagnosticLevel, RunEvent, RunEventKind, RunHandle};

pub struct RunEventStream {
    inner: BroadcastStream<RunEvent>,
    run: RunHandle,
    last_sequence: u64,
}

impl RunEventStream {
    pub(crate) fn new(
        run: RunHandle,
        receiver: tokio::sync::broadcast::Receiver<RunEvent>,
    ) -> Self {
        Self {
            inner: BroadcastStream::new(receiver),
            run,
            last_sequence: 0,
        }
    }
}

impl Stream for RunEventStream {
    type Item = RunEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                self.last_sequence = event.seq;
                Poll::Ready(Some(event))
            }
            Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(skipped)))) => {
                self.last_sequence = self.last_sequence.saturating_add(skipped);
                Poll::Ready(Some(RunEvent {
                    seq: self.last_sequence,
                    run_id: self.run.run_id,
                    agent_id: self.run.agent_id,
                    timestamp: std::time::SystemTime::now(),
                    kind: RunEventKind::Diagnostic(DiagnosticEvent {
                        level: DiagnosticLevel::Warning,
                        message: format!(
                            "event consumer lagged; {skipped} buffered events were dropped"
                        ),
                    }),
                    provider_meta: None,
                }))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
