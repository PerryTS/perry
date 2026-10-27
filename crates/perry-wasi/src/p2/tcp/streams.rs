//! Shared socket stream state. Native requests keep progressing when the guest
//! is not polling; only owned native work crosses to the Turnloop thread.
use crate::{
    p2::{SocketError, SocketResult},
    sockets::tcp::{Failure, TcpConnection},
};
use bytes::Bytes;
use perry_component_io::ResourceRequest;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use wasmtime_wasi_io::{
    poll::Pollable,
    streams::{
        DynInputStream, DynOutputStream, InputStream, OutputStream, StreamError, StreamResult,
    },
};

const CAPACITY: usize = crate::MAX_READ_SIZE_ALLOC;
type ReadTask = ResourceRequest<Result<Vec<u8>, Failure>>;
type UnitTask = ResourceRequest<Result<(), Failure>>;
fn stream_error(error: &Failure) -> StreamError {
    match error {
        Failure::Socket(code) | Failure::BrokenPipe(code) => {
            StreamError::LastOperationFailed((*code).into())
        }
        Failure::Driver(error) => StreamError::Trap(error.clone().into()),
    }
}
fn complete<T>(
    result: Result<Result<T, Failure>, perry_component_io::Error>,
) -> Result<T, Failure> {
    result.map_err(Failure::Driver)?
}
fn noop() -> Context<'static> {
    Context::from_waker(futures_util::task::noop_waker_ref())
}

enum ReadState {
    Idle,
    Waiting(ReadTask),
    Data(Bytes),
    Closing(UnitTask),
    Closed,
    Error(Failure),
}
struct Input {
    connection: TcpConnection,
    state: ReadState,
}
impl Input {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let next = match &mut self.state {
            ReadState::Waiting(task) => {
                match complete(std::task::ready!(Pin::new(task).poll(cx))) {
                    Ok(bytes) if bytes.is_empty() => ReadState::Closed,
                    Ok(bytes) => ReadState::Data(bytes.into()),
                    Err(error) => ReadState::Error(error),
                }
            }
            ReadState::Closing(task) => {
                match complete(std::task::ready!(Pin::new(task).poll(cx))) {
                    Ok(()) => ReadState::Closed,
                    Err(error) => ReadState::Error(error),
                }
            }
            _ => return Poll::Ready(()),
        };
        self.state = next;
        Poll::Ready(())
    }
    fn start(&mut self, size: usize) {
        self.state = match self.connection.read(size.min(CAPACITY)) {
            Ok(task) => ReadState::Waiting(task),
            Err(error) => ReadState::Error(Failure::Driver(error)),
        };
    }
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        let _ = self.poll(&mut noop());
        match &mut self.state {
            ReadState::Idle => {
                if size != 0 {
                    self.start(size);
                }
                Ok(Bytes::new())
            }
            ReadState::Waiting(_) => Ok(Bytes::new()),
            ReadState::Data(bytes) => {
                let result = bytes.split_to(bytes.len().min(size));
                if bytes.is_empty() {
                    self.state = ReadState::Idle;
                }
                Ok(result)
            }
            ReadState::Closing(_) | ReadState::Closed => Err(StreamError::Closed),
            ReadState::Error(error) => {
                let result = stream_error(error);
                if matches!(error, Failure::Socket(_) | Failure::BrokenPipe(_)) {
                    self.state = ReadState::Closed;
                }
                Err(result)
            }
        }
    }
    fn close(&mut self) -> SocketResult<()> {
        let _ = self.poll(&mut noop());
        if matches!(self.state, ReadState::Closing(_) | ReadState::Closed) {
            return Ok(());
        }
        let previous = std::mem::replace(&mut self.state, ReadState::Closed);
        let pending = if let ReadState::Waiting(task) = previous {
            Some(task)
        } else {
            None
        };
        match self.connection.close_receive(pending) {
            Ok(task) => {
                self.state = ReadState::Closing(task);
                Ok(())
            }
            Err(error) => {
                self.state = ReadState::Error(Failure::Driver(error.clone()));
                Err(SocketError::trap(error))
            }
        }
    }
}
struct Reader(Arc<Mutex<Input>>);
impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.0.lock().unwrap().close();
    }
}
#[async_trait::async_trait]
impl Pollable for Reader {
    async fn ready(&mut self) {
        std::future::poll_fn(|cx| {
            let mut input = self.0.lock().unwrap();
            if matches!(input.state, ReadState::Idle) {
                input.start(CAPACITY);
            }
            input.poll(cx)
        })
        .await;
    }
}
#[async_trait::async_trait]
impl InputStream for Reader {
    fn read(&mut self, size: usize) -> StreamResult<Bytes> {
        self.0.lock().unwrap().read(size)
    }
    async fn cancel(&mut self) {
        let _ = self.0.lock().unwrap().close();
    }
}

enum WriteState {
    Ready(usize),
    Waiting(UnitTask),
    Closing(UnitTask),
    Closed,
    Error(Failure),
}
struct Output {
    connection: TcpConnection,
    state: WriteState,
}
impl Output {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let next = match &mut self.state {
            WriteState::Waiting(task) => match complete(std::task::ready!(Pin::new(task).poll(cx)))
            {
                Ok(()) => WriteState::Ready(0),
                Err(error) => WriteState::Error(error),
            },
            WriteState::Closing(task) => match complete(std::task::ready!(Pin::new(task).poll(cx)))
            {
                Ok(()) => WriteState::Closed,
                Err(error) => WriteState::Error(error),
            },
            _ => return Poll::Ready(()),
        };
        self.state = next;
        Poll::Ready(())
    }
    fn error(&mut self) -> StreamResult<()> {
        match &self.state {
            WriteState::Closed => Err(StreamError::Closed),
            WriteState::Error(error) => {
                let result = stream_error(error);
                if matches!(error, Failure::Socket(_) | Failure::BrokenPipe(_)) {
                    self.state = WriteState::Closed;
                }
                Err(result)
            }
            _ => Ok(()),
        }
    }
    fn check(&mut self) -> StreamResult<usize> {
        let _ = self.poll(&mut noop());
        self.error()?;
        match &mut self.state {
            WriteState::Ready(permit) => {
                *permit = CAPACITY;
                Ok(CAPACITY)
            }
            _ => Ok(0),
        }
    }
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.error()?;
        let WriteState::Ready(permit) = &mut self.state else {
            return Err(StreamError::Trap(wasmtime::format_err!(
                "TCP write requires readiness"
            )));
        };
        if bytes.len() > *permit {
            return Err(StreamError::Trap(wasmtime::format_err!(
                "TCP write exceeds permit"
            )));
        }
        *permit -= bytes.len();
        if bytes.is_empty() {
            return Ok(());
        }
        self.state = WriteState::Waiting(
            self.connection
                .write(bytes.to_vec())
                .map_err(|e| StreamError::Trap(e.into()))?,
        );
        Ok(())
    }
    fn flush(&mut self) -> StreamResult<()> {
        let _ = self.poll(&mut noop());
        self.error()?;
        if let WriteState::Ready(permit) = &mut self.state {
            *permit = 0;
        }
        // Every owned write already includes native flush. A following ready
        // waits for that same completion rather than reporting false readiness.
        Ok(())
    }
    fn close(&mut self) -> SocketResult<()> {
        let _ = self.poll(&mut noop());
        if matches!(self.state, WriteState::Closing(_) | WriteState::Closed) {
            return Ok(());
        }
        let previous = std::mem::replace(&mut self.state, WriteState::Closed);
        let pending = if let WriteState::Waiting(task) = previous {
            Some(task)
        } else {
            None
        };
        match self.connection.close_send(pending) {
            Ok(task) => {
                self.state = WriteState::Closing(task);
                Ok(())
            }
            Err(error) => {
                self.state = WriteState::Error(Failure::Driver(error.clone()));
                Err(SocketError::trap(error))
            }
        }
    }
}
struct Writer(Arc<Mutex<Output>>);
impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.0.lock().unwrap().close();
    }
}
#[async_trait::async_trait]
impl Pollable for Writer {
    async fn ready(&mut self) {
        std::future::poll_fn(|cx| self.0.lock().unwrap().poll(cx)).await;
    }
}
#[async_trait::async_trait]
impl OutputStream for Writer {
    fn check_write(&mut self) -> StreamResult<usize> {
        self.0.lock().unwrap().check()
    }
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.0.lock().unwrap().write(bytes)
    }
    fn flush(&mut self) -> StreamResult<()> {
        self.0.lock().unwrap().flush()
    }
    async fn cancel(&mut self) {
        let _ = self.0.lock().unwrap().close();
    }
}

pub(super) struct Streams {
    input: Arc<Mutex<Input>>,
    output: Arc<Mutex<Output>>,
}
impl Streams {
    pub(super) fn new(connection: TcpConnection) -> (Self, DynInputStream, DynOutputStream) {
        let input = Arc::new(Mutex::new(Input {
            connection: connection.clone(),
            state: ReadState::Idle,
        }));
        let output = Arc::new(Mutex::new(Output {
            connection,
            state: WriteState::Ready(0),
        }));
        (
            Self {
                input: input.clone(),
                output: output.clone(),
            },
            Box::new(Reader(input)),
            Box::new(Writer(output)),
        )
    }
    pub(super) fn shutdown(&mut self, receive: bool, send: bool) -> SocketResult<()> {
        if receive {
            self.input.lock().unwrap().close()?;
        }
        if send {
            self.output.lock().unwrap().close()?;
        }
        Ok(())
    }
}
