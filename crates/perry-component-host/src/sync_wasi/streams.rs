use super::{
    bindings::wasi::{
        cli,
        io::{poll, streams},
    },
    errors::IoError,
    State,
};
use cap_std::fs::File;
use std::{
    io::{self, Read, Write},
    sync::Arc,
};
use system_interface::fs::FileIoExt;
use wasmtime::component::{Resource, ResourceTableError};

const PERMIT: u64 = 64 * 1024;
pub enum Input {
    Tcp(Arc<super::tcp_io::Connection>),
    Empty,
    Stdin,
    File { file: Arc<File>, offset: u64 },
    Closed,
}
pub enum Output {
    Tcp(Arc<super::tcp_io::Connection>),
    Sink,
    Stdout,
    Stderr,
    Captured(perry_wasi::pipe::MemoryOutputPipe),
    File {
        file: Arc<File>,
        offset: Option<u64>,
    },
    Closed,
}
pub enum Pollable {
    Always,
    Operation(Arc<super::native_io::Operation>),
    Resolve(u32),
    UdpReceive(u32),
    UdpSend(u32),
    TcpSocket(u32),
    TcpInput(u32),
    TcpOutput(u32),
}
impl Input {
    pub fn file(file: Arc<File>, offset: u64) -> Self {
        Self::File { file, offset }
    }
}
impl Output {
    pub fn file(file: Arc<File>, offset: Option<u64>) -> Self {
        Self::File { file, offset }
    }
}
#[derive(Debug)]
pub enum StreamError {
    Closed,
    Failed(io::Error),
    Trap(wasmtime::Error),
}
impl From<ResourceTableError> for StreamError {
    fn from(value: ResourceTableError) -> Self {
        Self::Trap(value.into())
    }
}
impl From<io::Error> for StreamError {
    fn from(value: io::Error) -> Self {
        Self::Failed(value)
    }
}
impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StreamError {}
type StreamResult<T> = Result<T, StreamError>;
fn trap(message: &str) -> StreamError {
    StreamError::Trap(wasmtime::format_err!("{message}"))
}

impl streams::Host for State {
    fn convert_stream_error(
        &mut self,
        error: StreamError,
    ) -> wasmtime::Result<streams::StreamError> {
        match error {
            StreamError::Closed => Ok(streams::StreamError::Closed),
            StreamError::Failed(error) => Ok(streams::StreamError::LastOperationFailed(
                self.table.push(IoError(error))?,
            )),
            StreamError::Trap(error) => Err(error),
        }
    }
}
impl streams::HostInputStream for State {
    fn drop(&mut self, stream: Resource<Input>) -> wasmtime::Result<()> {
        if let Input::Tcp(c) = self.table.delete(stream)? {
            c.close_read();
        }
        Ok(())
    }
    fn read(&mut self, stream: Resource<Input>, length: u64) -> StreamResult<Vec<u8>> {
        if let Input::Tcp(c) = self.table.get(&stream)? {
            return c.read(self.io, length.min(PERMIT) as usize, false);
        }
        let input = self.table.get_mut(&stream)?;
        if matches!(input, Input::Empty | Input::Closed) {
            return Err(StreamError::Closed);
        }
        let mut bytes = vec![0; length.min(PERMIT) as usize];
        if bytes.is_empty() {
            return Ok(bytes);
        }
        let result = match input {
            Input::Stdin => io::stdin().read(&mut bytes),
            Input::File { file, offset } => file.read_at(&mut bytes, *offset).and_then(|n| {
                *offset = offset
                    .checked_add(n as u64)
                    .ok_or(io::ErrorKind::InvalidInput)?;
                Ok(n)
            }),
            Input::Empty | Input::Closed | Input::Tcp(_) => unreachable!(),
        };
        match result {
            Ok(0) => {
                *input = Input::Closed;
                Err(StreamError::Closed)
            }
            Ok(n) => {
                bytes.truncate(n);
                Ok(bytes)
            }
            Err(e) => {
                *input = Input::Closed;
                Err(e.into())
            }
        }
    }
    fn blocking_read(&mut self, stream: Resource<Input>, length: u64) -> StreamResult<Vec<u8>> {
        if let Input::Tcp(c) = self.table.get(&stream)? {
            return c.read(self.io, length.min(PERMIT) as usize, true);
        }
        self.read(stream, length)
    }
    fn skip(&mut self, stream: Resource<Input>, length: u64) -> StreamResult<u64> {
        Ok(self.read(stream, length)?.len() as u64)
    }
    fn blocking_skip(&mut self, stream: Resource<Input>, length: u64) -> StreamResult<u64> {
        Ok(self.blocking_read(stream, length)?.len() as u64)
    }
    fn subscribe(&mut self, stream: Resource<Input>) -> wasmtime::Result<Resource<Pollable>> {
        let p = if matches!(self.table.get(&stream)?, Input::Tcp(_)) {
            Pollable::TcpInput(stream.rep())
        } else {
            Pollable::Always
        };
        Ok(self.table.push_child(p, &stream)?)
    }
}
impl streams::HostOutputStream for State {
    fn drop(&mut self, stream: Resource<Output>) -> wasmtime::Result<()> {
        if let Output::Tcp(c) = self.table.delete(stream)? {
            c.close_write(self.io);
        }
        Ok(())
    }
    fn check_write(&mut self, stream: Resource<Output>) -> StreamResult<u64> {
        if let Output::Tcp(c) = self.table.get(&stream)? {
            return c.check_write(self.io);
        }
        if let Output::Captured(buffer) = self.table.get_mut(&stream)? {
            return wasmtime_wasi_io::streams::OutputStream::check_write(buffer)
                .map(|size| (size as u64).min(PERMIT))
                .map_err(captured_error);
        }
        if matches!(self.table.get(&stream)?, Output::Closed) {
            Err(StreamError::Closed)
        } else {
            Ok(PERMIT)
        }
    }
    fn write(&mut self, stream: Resource<Output>, contents: Vec<u8>) -> StreamResult<()> {
        if contents.len() as u64 > PERMIT {
            return Err(trap("write exceeds check-write permit"));
        }
        let output = self.table.get_mut(&stream)?;
        let result = match output {
            Output::Closed => return Err(StreamError::Closed),
            Output::Tcp(c) => return c.write(self.io, contents),
            Output::Captured(buffer) => {
                return wasmtime_wasi_io::streams::OutputStream::write(buffer, contents.into())
                    .map_err(captured_error);
            }
            Output::Sink => Ok(()),
            Output::Stdout => io::stdout().write_all(&contents),
            Output::Stderr => io::stderr().write_all(&contents),
            Output::File { file, offset: None } => file.append_all(&contents),
            Output::File {
                file,
                offset: Some(offset),
            } => {
                let mut remaining = contents.as_slice();
                let mut result = Ok(());
                while !remaining.is_empty() {
                    match file.write_at(remaining, *offset) {
                        Ok(0) => {
                            result = Err(io::ErrorKind::WriteZero.into());
                            break;
                        }
                        Ok(n) => {
                            match offset.checked_add(n as u64) {
                                Some(next) => *offset = next,
                                None => {
                                    result = Err(io::ErrorKind::InvalidInput.into());
                                    break;
                                }
                            }
                            remaining = &remaining[n..];
                        }
                        Err(e) => {
                            result = Err(e);
                            break;
                        }
                    }
                }
                result
            }
        };
        if result.is_err() {
            *output = Output::Closed;
        }
        result.map_err(Into::into)
    }
    fn flush(&mut self, stream: Resource<Output>) -> StreamResult<()> {
        let output = self.table.get_mut(&stream)?;
        let result = match output {
            Output::Closed => return Err(StreamError::Closed),
            Output::Tcp(c) => return c.prepare_write(self.io).map_err(StreamError::Trap),
            Output::Stdout => io::stdout().flush(),
            Output::Stderr => io::stderr().flush(),
            Output::Sink | Output::File { .. } | Output::Captured(_) => Ok(()),
        };
        if result.is_err() {
            *output = Output::Closed;
        }
        result.map_err(Into::into)
    }
    fn blocking_flush(&mut self, stream: Resource<Output>) -> StreamResult<()> {
        if let Output::Tcp(c) = self.table.get(&stream)? {
            return c.wait_write(self.io);
        }
        self.flush(stream)
    }
    fn blocking_write_and_flush(
        &mut self,
        stream: Resource<Output>,
        contents: Vec<u8>,
    ) -> StreamResult<()> {
        if contents.len() > 4096 {
            return Err(trap("blocking-write-and-flush exceeds 4096 bytes"));
        }
        if let Output::Tcp(c) = self.table.get(&stream)? {
            c.wait_write(self.io)?;
        }
        self.write(Resource::new_borrow(stream.rep()), contents)?;
        self.blocking_flush(stream)
    }
    fn subscribe(&mut self, stream: Resource<Output>) -> wasmtime::Result<Resource<Pollable>> {
        let p = if matches!(self.table.get(&stream)?, Output::Tcp(_)) {
            Pollable::TcpOutput(stream.rep())
        } else {
            Pollable::Always
        };
        Ok(self.table.push_child(p, &stream)?)
    }
    fn write_zeroes(&mut self, stream: Resource<Output>, length: u64) -> StreamResult<()> {
        if length > PERMIT {
            return Err(trap("write-zeroes exceeds check-write permit"));
        }
        self.write(stream, vec![0; length as usize])
    }
    fn blocking_write_zeroes_and_flush(
        &mut self,
        stream: Resource<Output>,
        length: u64,
    ) -> StreamResult<()> {
        if length > 4096 {
            return Err(trap("blocking-write-zeroes-and-flush exceeds 4096 bytes"));
        }
        self.blocking_write_and_flush(stream, vec![0; length as usize])
    }
    fn splice(
        &mut self,
        stream: Resource<Output>,
        input: Resource<Input>,
        length: u64,
    ) -> StreamResult<u64> {
        let permit = self.check_write(Resource::new_borrow(stream.rep()))?;
        let bytes = streams::HostInputStream::read(self, input, length.min(permit))?;
        let n = bytes.len();
        self.write(stream, bytes)?;
        Ok(n as u64)
    }
    fn blocking_splice(
        &mut self,
        stream: Resource<Output>,
        input: Resource<Input>,
        length: u64,
    ) -> StreamResult<u64> {
        if let Output::Tcp(c) = self.table.get(&stream)? {
            c.wait_write(self.io)?;
        }
        let permit = self.check_write(Resource::new_borrow(stream.rep()))?;
        let bytes = streams::HostInputStream::blocking_read(self, input, length.min(permit))?;
        let n = bytes.len();
        self.write(stream, bytes)?;
        Ok(n as u64)
    }
}
pub enum PollOperation<'a> {
    Borrowed(&'a super::native_io::Operation),
    Shared(Arc<super::native_io::Operation>),
}
impl std::ops::Deref for PollOperation<'_> {
    type Target = super::native_io::Operation;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(op) => op,
            Self::Shared(op) => op,
        }
    }
}
impl State {
    fn prepare_poll(&mut self, pollable: &Resource<Pollable>) -> wasmtime::Result<()> {
        match *self.table.get(pollable)? {
            Pollable::UdpReceive(rep) => self
                .table
                .get_mut(&Resource::<super::udp::Incoming>::new_borrow(rep))?
                .prepare(self.io)?,
            Pollable::TcpSocket(rep) => self
                .table
                .get_mut(&Resource::<super::tcp::TcpSocket>::new_borrow(rep))?
                .prepare(self.io)?,
            Pollable::TcpInput(rep) => {
                if let Input::Tcp(c) = self.table.get(&Resource::<Input>::new_borrow(rep))? {
                    c.prepare_read(self.io)?;
                }
            }
            Pollable::TcpOutput(rep) => {
                if let Output::Tcp(c) = self.table.get(&Resource::<Output>::new_borrow(rep))? {
                    c.prepare_write(self.io)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn poll_is_ready(&self, pollable: &Resource<Pollable>) -> wasmtime::Result<bool> {
        let value = self.table.get(pollable)?;
        if let Pollable::UdpReceive(rep) = value {
            return Ok(matches!(
                self.table
                    .get(&Resource::<super::udp::Incoming>::new_borrow(*rep))?
                    .state,
                super::udp::Receive::Ready(_)
            ));
        }
        Ok(self.poll_operation(value)?.is_none_or(|op| op.ready()))
    }
    fn poll_operation<'a>(
        &'a self,
        pollable: &'a Pollable,
    ) -> wasmtime::Result<Option<PollOperation<'a>>> {
        match pollable {
            Pollable::Always => Ok(None),
            Pollable::Operation(op) => Ok(Some(PollOperation::Borrowed(op))),
            Pollable::TcpSocket(rep) => Ok(self
                .table
                .get(&Resource::<super::tcp::TcpSocket>::new_borrow(*rep))?
                .operation()
                .map(PollOperation::Borrowed)),
            Pollable::TcpInput(rep) => Ok(
                match self.table.get(&Resource::<Input>::new_borrow(*rep))? {
                    Input::Tcp(c) => c.read_operation().map(PollOperation::Shared),
                    _ => None,
                },
            ),
            Pollable::TcpOutput(rep) => Ok(
                match self.table.get(&Resource::<Output>::new_borrow(*rep))? {
                    Output::Tcp(c) => c.write_operation().map(PollOperation::Shared),
                    _ => None,
                },
            ),
            Pollable::UdpReceive(rep) => {
                let stream = self
                    .table
                    .get(&Resource::<super::udp::Incoming>::new_borrow(*rep))?;
                Ok(match &stream.state {
                    super::udp::Receive::Pending(op) => Some(PollOperation::Borrowed(op)),
                    _ => None,
                })
            }
            Pollable::UdpSend(rep) => Ok(self
                .table
                .get(&Resource::<super::udp::Outgoing>::new_borrow(*rep))?
                .pending
                .as_ref()
                .map(PollOperation::Borrowed)),
            Pollable::Resolve(rep) => {
                let stream = self
                    .table
                    .get(&Resource::<super::network::ResolveStream>::new_borrow(*rep))?;
                Ok(match &stream.0 {
                    super::network::ResolveState::Pending(op) => Some(PollOperation::Borrowed(op)),
                    _ => None,
                })
            }
        }
    }
}
impl poll::HostPollable for State {
    fn ready(&mut self, pollable: Resource<Pollable>) -> wasmtime::Result<bool> {
        self.prepare_poll(&pollable)?;
        if let Some(op) = self.poll_operation(self.table.get(&pollable)?)? {
            self.io.poll(&[&op], false)?;
        }
        self.prepare_poll(&pollable)?;
        self.poll_is_ready(&pollable)
    }

    fn block(&mut self, pollable: Resource<Pollable>) -> wasmtime::Result<()> {
        poll::Host::poll(self, vec![pollable])?;
        Ok(())
    }
    fn drop(&mut self, pollable: Resource<Pollable>) -> wasmtime::Result<()> {
        self.table.delete(pollable)?;
        Ok(())
    }
}
impl poll::Host for State {
    fn poll(&mut self, pollables: Vec<Resource<Pollable>>) -> wasmtime::Result<Vec<u32>> {
        wasmtime::ensure!(!pollables.is_empty(), "poll requires at least one pollable");
        wasmtime::ensure!(pollables.len() <= u32::MAX as usize, "too many pollables");
        loop {
            for p in &pollables {
                self.prepare_poll(p)?;
            }
            {
                let sources = pollables
                    .iter()
                    .map(|p| self.poll_operation(self.table.get(p)?))
                    .collect::<wasmtime::Result<Vec<_>>>()?;
                let operations = sources
                    .iter()
                    .filter_map(|p| p.as_ref().map(|op| &**op))
                    .collect::<Vec<_>>();
                self.io.poll(&operations, false)?;
            }
            for p in &pollables {
                self.prepare_poll(p)?;
            }
            let mut ready = Vec::new();
            for (i, p) in pollables.iter().enumerate() {
                if self.poll_is_ready(p)? {
                    ready.push(i as u32);
                }
            }
            if !ready.is_empty() {
                return Ok(ready);
            }
            let sources = pollables
                .iter()
                .map(|p| self.poll_operation(self.table.get(p)?))
                .collect::<wasmtime::Result<Vec<_>>>()?;
            let operations = sources
                .iter()
                .filter_map(|p| p.as_ref().map(|op| &**op))
                .collect::<Vec<_>>();
            self.io.poll(&operations, true)?;
        }
    }
}
impl cli::stdin::Host for State {
    fn get_stdin(&mut self) -> wasmtime::Result<Resource<Input>> {
        Ok(self.table.push(if self.options.inherit_stdin {
            Input::Stdin
        } else {
            Input::Empty
        })?)
    }
}
impl cli::stdout::Host for State {
    fn get_stdout(&mut self) -> wasmtime::Result<Resource<Output>> {
        Ok(self.table.push(if self.options.inherit_stdout {
            Output::Stdout
        } else {
            Output::Sink
        })?)
    }
}
impl cli::stderr::Host for State {
    fn get_stderr(&mut self) -> wasmtime::Result<Resource<Output>> {
        Ok(self
            .table
            .push(if let Some(buffer) = &self.options.stderr_buffer {
                Output::Captured(buffer.clone())
            } else if self.options.inherit_stderr {
                Output::Stderr
            } else {
                Output::Sink
            })?)
    }
}

fn captured_error(error: wasmtime_wasi_io::streams::StreamError) -> StreamError {
    match error {
        wasmtime_wasi_io::streams::StreamError::Closed => StreamError::Closed,
        wasmtime_wasi_io::streams::StreamError::Trap(error) => StreamError::Trap(error),
        wasmtime_wasi_io::streams::StreamError::LastOperationFailed(error) => {
            StreamError::Failed(io::Error::other(error.to_string()))
        }
    }
}
