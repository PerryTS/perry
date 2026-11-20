//! Native stdio is driven on the context's Turnloop owner. Each operation owns
//! its duplicate and buffer, and closes the duplicate after completion. There
//! is no blocking read thread or ambient async runtime. Stdout/stderr expose
//! one bounded outstanding write per guest stream; stdin views share one read.
use super::{IsTerminal, StdinStream, StdoutStream};
use bytes::Bytes;
use futures_util::{
    future::{BoxFuture, Shared},
    io::{AsyncReadExt, AsyncWriteExt},
    FutureExt,
};
use perry_component_io::{Request, Submitter};
use std::{
    io::IsTerminal as _,
    sync::{Arc, Mutex},
};
use turnloop::Stdio;
use wasmtime_wasi_io::{
    poll::Pollable,
    streams::{InputStream, OutputStream, StreamError, StreamResult},
};

const CAPACITY: usize = crate::MAX_READ_SIZE_ALLOC;
#[derive(Clone, Debug)]
enum Failure {
    Driver(perry_component_io::Error),
    Native {
        kind: std::io::ErrorKind,
        raw: Option<i32>,
        message: String,
    },
}
impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        if let Some(error) = e
            .get_ref()
            .and_then(|e| e.downcast_ref::<turnloop::Error>())
        {
            return native_error(*error);
        }
        Self::Native {
            kind: e.kind(),
            raw: e.raw_os_error(),
            message: e.to_string(),
        }
    }
}
impl Failure {
    fn stream(&self) -> StreamError {
        match self {
            Self::Driver(e) => StreamError::Trap(e.clone().into()),
            Self::Native {
                kind: std::io::ErrorKind::BrokenPipe,
                ..
            } => StreamError::Closed,
            Self::Native { kind, raw, message } => StreamError::LastOperationFailed(
                raw.map(std::io::Error::from_raw_os_error)
                    .unwrap_or_else(|| std::io::Error::new(*kind, message.clone()))
                    .into(),
            ),
        }
    }
}
fn native_error(e: turnloop::Error) -> Failure {
    use std::io::ErrorKind as I;
    use turnloop::ErrorKind as T;
    if let Some(os) = e.os {
        return std::io::Error::from_raw_os_error(os).into();
    }
    let kind = match e.kind {
        T::Cancelled => return Failure::Driver(perry_component_io::Error::Cancelled),
        T::ResourceLimit => return Failure::Driver(perry_component_io::Error::Capacity),
        T::Unsupported => I::Unsupported,
        T::InvalidInput => I::InvalidInput,
        T::NotFound => I::NotFound,
        T::WouldBlock => I::WouldBlock,
        T::TimedOut => I::TimedOut,
        T::ConnectionRefused => I::ConnectionRefused,
        T::ConnectionReset => I::ConnectionReset,
        T::BrokenPipe => I::BrokenPipe,
        T::PermissionDenied => I::PermissionDenied,
        T::AlreadyExists => I::AlreadyExists,
        T::NotADirectory => I::NotADirectory,
        T::IsADirectory => I::IsADirectory,
        T::DirectoryNotEmpty => I::DirectoryNotEmpty,
        T::Other => I::Other,
    };
    Failure::Native {
        kind,
        raw: None,
        message: e.to_string(),
    }
}

#[derive(Clone)]
struct ReadTask {
    result: Shared<BoxFuture<'static, Result<Bytes, Failure>>>,
    cancellation: perry_component_io::Cancellation,
}
enum ReadState {
    Idle,
    Waiting(ReadTask),
    Data(Bytes),
    Error(Failure),
    Closed,
}
struct SharedInput {
    state: ReadState,
    views: usize,
}
pub struct NativeInput {
    submitter: Submitter,
    shared: Arc<Mutex<SharedInput>>,
    terminal: bool,
}
impl NativeInput {
    pub fn stdin(submitter: Submitter) -> Self {
        Self {
            submitter,
            shared: Arc::new(Mutex::new(SharedInput {
                state: ReadState::Idle,
                views: 0,
            })),
            terminal: std::io::stdin().is_terminal(),
        }
    }
}
impl IsTerminal for NativeInput {
    fn is_terminal(&self) -> bool {
        self.terminal
    }
}
impl StdinStream for NativeInput {
    fn p2_stream(&self) -> Box<dyn InputStream> {
        self.shared.lock().unwrap().views += 1;
        Box::new(InputView {
            submitter: self.submitter.clone(),
            shared: self.shared.clone(),
            closed: false,
        })
    }
}
struct InputView {
    submitter: Submitter,
    shared: Arc<Mutex<SharedInput>>,
    closed: bool,
}
impl InputView {
    fn start(&self, state: &mut ReadState, size: usize) {
        let request = self.submitter.submit(move |io| async move {
            let handle = io.driver().open_stdio(Stdio::Stdin).map_err(native_error)?;
            let mut stream = io.io(handle);
            let mut bytes = vec![0; size.min(CAPACITY)];
            let result = stream.read(&mut bytes).await.map_err(Failure::from);
            let close = io.close(handle).await.map_err(native_error);
            let len = result?;
            close?;
            bytes.truncate(len);
            Ok(Bytes::from(bytes))
        });
        *state = match request {
            Ok(task) => {
                let cancellation = task.cancellation();
                ReadState::Waiting(ReadTask {
                    cancellation,
                    result: async move { task.await.map_err(Failure::Driver)? }
                        .boxed()
                        .shared(),
                })
            }
            Err(e) => ReadState::Error(Failure::Driver(e)),
        };
    }
    fn finish(state: &mut ReadState) {
        if let ReadState::Waiting(task) = state {
            if let Some(result) = task.result.clone().now_or_never() {
                *state = match result {
                    Ok(bytes) if bytes.is_empty() => ReadState::Closed,
                    Ok(bytes) => ReadState::Data(bytes),
                    Err(e) => ReadState::Error(e),
                };
            }
        }
    }
    fn close(&mut self) -> Option<ReadTask> {
        if self.closed {
            return None;
        }
        self.closed = true;
        let mut shared = self.shared.lock().unwrap();
        shared.views -= 1;
        if shared.views == 0 {
            // Preserve already-received bytes for later handles. Cancel an
            // unread pending operation when there are no remaining views.
            Self::finish(&mut shared.state);
            if matches!(shared.state, ReadState::Waiting(_)) {
                if let ReadState::Waiting(task) =
                    std::mem::replace(&mut shared.state, ReadState::Idle)
                {
                    task.cancellation.cancel();
                    return Some(task);
                }
            }
        }
        None
    }
}
impl Drop for InputView {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
#[async_trait::async_trait]
impl Pollable for InputView {
    async fn ready(&mut self) {
        if self.closed {
            return;
        }
        let task = {
            let mut shared = self.shared.lock().unwrap();
            Self::finish(&mut shared.state);
            if matches!(shared.state, ReadState::Idle) {
                self.start(&mut shared.state, 4096);
            }
            match &shared.state {
                ReadState::Waiting(task) => Some(task.clone()),
                _ => None,
            }
        };
        if let Some(task) = task {
            let _ = task.result.await;
            Self::finish(&mut self.shared.lock().unwrap().state);
        }
    }
}
#[async_trait::async_trait]
impl InputStream for InputView {
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        if self.closed {
            return Err(StreamError::Closed);
        }
        let mut shared = self.shared.lock().unwrap();
        Self::finish(&mut shared.state);
        match &mut shared.state {
            ReadState::Idle => {
                if size != 0 {
                    self.start(&mut shared.state, size);
                }
                Ok(Bytes::new())
            }
            ReadState::Waiting(_) => Ok(Bytes::new()),
            ReadState::Data(bytes) => {
                let result = bytes.split_to(size.min(bytes.len()));
                if bytes.is_empty() {
                    shared.state = ReadState::Idle;
                }
                Ok(result)
            }
            ReadState::Error(e) => {
                let result = e.stream();
                if matches!(e, Failure::Native { .. }) {
                    shared.state = ReadState::Closed;
                }
                Err(result)
            }
            ReadState::Closed => Err(StreamError::Closed),
        }
    }
    async fn cancel(&mut self) {
        if let Some(task) = self.close() {
            let _ = task.result.await;
        }
    }
}

pub struct NativeOutput {
    submitter: Submitter,
    which: Stdio,
    terminal: bool,
}
impl NativeOutput {
    pub fn stdout(submitter: Submitter) -> Self {
        Self {
            submitter,
            which: Stdio::Stdout,
            terminal: std::io::stdout().is_terminal(),
        }
    }
    pub fn stderr(submitter: Submitter) -> Self {
        Self {
            submitter,
            which: Stdio::Stderr,
            terminal: std::io::stderr().is_terminal(),
        }
    }
}
impl IsTerminal for NativeOutput {
    fn is_terminal(&self) -> bool {
        self.terminal
    }
}
impl StdoutStream for NativeOutput {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(OutputView {
            submitter: self.submitter.clone(),
            which: self.which,
            state: WriteState::Ready,
        })
    }
}
enum WriteState {
    Ready,
    Waiting(Request<Result<(), Failure>>),
    Error(Failure),
    Closed,
}
struct OutputView {
    submitter: Submitter,
    which: Stdio,
    state: WriteState,
}
impl OutputView {
    fn finish(&mut self) {
        if let WriteState::Waiting(task) = &mut self.state {
            if let Some(result) = task.now_or_never() {
                self.state = match result {
                    Ok(Ok(())) => WriteState::Ready,
                    Ok(Err(e)) => WriteState::Error(e),
                    Err(e) => WriteState::Error(Failure::Driver(e)),
                };
            }
        }
    }
}
#[async_trait::async_trait]
impl Pollable for OutputView {
    async fn ready(&mut self) {
        if let WriteState::Waiting(task) = &mut self.state {
            self.state = match task.await {
                Ok(Ok(())) => WriteState::Ready,
                Ok(Err(e)) => WriteState::Error(e),
                Err(e) => WriteState::Error(Failure::Driver(e)),
            };
        }
    }
}
#[async_trait::async_trait]
impl OutputStream for OutputView {
    fn check_write(&mut self) -> StreamResult<usize> {
        self.finish();
        match &self.state {
            WriteState::Ready => Ok(CAPACITY),
            WriteState::Waiting(_) => Ok(0),
            WriteState::Error(e) => {
                let result = e.stream();
                if matches!(e, Failure::Native { .. }) {
                    self.state = WriteState::Closed;
                }
                Err(result)
            }
            WriteState::Closed => Err(StreamError::Closed),
        }
    }
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        let permitted = self.check_write()?;
        if permitted == 0 || bytes.len() > permitted {
            return Err(StreamError::Trap(wasmtime::format_err!(
                "stdio write exceeds permit"
            )));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let which = self.which;
        self.state = WriteState::Waiting(
            self.submitter
                .submit(move |io| async move {
                    let handle = io.driver().open_stdio(which).map_err(native_error)?;
                    let mut stream = io.io(handle);
                    let result = async {
                        stream.write_all(&bytes).await?;
                        stream.flush().await
                    }
                    .await
                    .map_err(Failure::from);
                    let close = io.close(handle).await.map_err(native_error);
                    result?;
                    close
                })
                .map_err(|e| StreamError::Trap(e.into()))?,
        );
        Ok(())
    }
    fn flush(&mut self) -> StreamResult<()> {
        self.finish();
        match &self.state {
            WriteState::Error(e) => {
                let result = e.stream();
                if matches!(e, Failure::Native { .. }) {
                    self.state = WriteState::Closed;
                }
                Err(result)
            }
            WriteState::Closed => Err(StreamError::Closed),
            _ => Ok(()),
        }
    }
    async fn cancel(&mut self) {
        if let WriteState::Waiting(task) = std::mem::replace(&mut self.state, WriteState::Closed) {
            task.cancel();
            let _ = task.await;
        }
    }
}
