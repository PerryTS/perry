//! Demand-driven directory batches. The cursor and metadata syscalls stay on
//! Turnloop's bounded blocking pool; no Store or guest memory crosses a wait.
use super::types::{DirectoryEntry, ErrorCode};
use crate::filesystem::{primitives, Dir};
use futures_channel::oneshot;
use futures_util::future::BoxFuture;
use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::{
    component::{Destination, StreamProducer, StreamResult},
    StoreContextMut,
};

type Cursor = Box<dyn Iterator<Item = std::io::Result<primitives::DirEntry>> + Send>;
type Outcome = oneshot::Sender<Result<(), ErrorCode>>;
struct Batch {
    cursor: Option<Cursor>,
    entries: VecDeque<DirectoryEntry>,
    end: Option<Result<(), ErrorCode>>,
}
pub(super) struct Directory {
    dir: Dir,
    batch: Option<Batch>,
    pending: Option<BoxFuture<'static, Result<Batch, perry_component_io::Error>>>,
    outcome: Option<Outcome>,
}
impl Directory {
    pub fn new(dir: Dir, outcome: Outcome) -> Self {
        Self {
            dir,
            batch: Some(Batch {
                cursor: None,
                entries: VecDeque::new(),
                end: None,
            }),
            pending: None,
            outcome: Some(outcome),
        }
    }
}
impl<D> StreamProducer<D> for Directory {
    type Item = DirectoryEntry;
    type Buffer = Option<DirectoryEntry>;
    fn poll_produce<'a>(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut store: StoreContextMut<'a, D>,
        mut dst: Destination<'a, Self::Item, Self::Buffer>,
        finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        if dst.remaining(&mut store) == Some(0) {
            return Poll::Ready(Ok(StreamResult::Completed));
        }
        loop {
            if let Some(batch) = self.batch.as_mut() {
                if let Some(entry) = batch.entries.pop_front() {
                    dst.set_buffer(Some(entry));
                    return Poll::Ready(Ok(StreamResult::Completed));
                }
                if let Some(end) = batch.end.take() {
                    if let Some(outcome) = self.outcome.take() {
                        let _ = outcome.send(end);
                    }
                    return Poll::Ready(Ok(StreamResult::Dropped));
                }
                let batch = self.batch.take().unwrap();
                let dir = self.dir.clone();
                self.pending = Some(Box::pin(async move {
                    dir.run_blocking(move |dir| read_batch(dir, batch)).await
                }));
            }
            match self.pending.as_mut().unwrap().as_mut().poll(cx) {
                Poll::Ready(Ok(batch)) => {
                    self.pending = None;
                    self.batch = Some(batch);
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error.into())),
                Poll::Pending if finish => return Poll::Ready(Ok(StreamResult::Cancelled)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
fn read_batch(dir: &std::fs::File, mut batch: Batch) -> Batch {
    if batch.cursor.is_none() {
        match primitives::read_base_dir(dir) {
            Ok(cursor) => batch.cursor = Some(Box::new(cursor)),
            Err(error) => {
                batch.end = Some(Err(error.into()));
                return batch;
            }
        }
    }
    // Bound both successful entries and skipped entries per native submission.
    for _ in 0..64 {
        let Some(entry) = batch.cursor.as_mut().unwrap().next() else {
            batch.end = Some(Ok(()));
            batch.cursor = None;
            break;
        };
        match map_entry(entry) {
            Ok(Some(entry)) => batch.entries.push_back(entry),
            Ok(None) => {}
            Err(error) => {
                batch.end = Some(Err(error));
                batch.cursor = None;
                break;
            }
        }
    }
    batch
}
fn map_entry(
    entry: std::io::Result<primitives::DirEntry>,
) -> Result<Option<DirectoryEntry>, ErrorCode> {
    let entry = entry.and_then(|entry| Ok((entry.metadata()?, entry.file_name())));
    match entry {
        Ok((metadata, name)) => Ok(Some(DirectoryEntry {
            type_: metadata.file_type().into(),
            name: name
                .into_string()
                .map_err(|_| ErrorCode::IllegalByteSequence)?,
        })),
        Err(error) => {
            #[cfg(windows)]
            {
                use windows_sys::Win32::Foundation::{
                    ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION,
                };
                if matches!(error.raw_os_error(), Some(code) if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_SHARING_VIOLATION as i32)
                {
                    return Ok(None);
                }
            }
            Err(error.into())
        }
    }
}
