use super::native_io::{self, Client, Operation, Socket};
use super::streams::StreamError;
use perry_abi::component_io as abi;
use std::{
    io,
    sync::{Arc, Mutex},
};
pub struct Connection {
    pub socket: Arc<Socket>,
    pub read: Mutex<Read>,
    pub write: Mutex<Write>,
}
pub enum Read {
    Idle,
    Pending(Arc<Operation>),
    Bytes { data: Vec<u8>, offset: usize },
    Failed(io::Error),
    Closed,
}
pub enum Write {
    Ready,
    Pending(Arc<Operation>),
    Failed(io::Error),
    Closed,
}
pub fn io_error(e: native_io::Error) -> io::Error {
    if e.0.os != 0 {
        return io::Error::from_raw_os_error(e.0.os);
    }
    let kind = match e.0.kind {
        abi::ERR_WOULD_BLOCK => io::ErrorKind::WouldBlock,
        abi::ERR_TIMEOUT => io::ErrorKind::TimedOut,
        abi::ERR_REFUSED => io::ErrorKind::ConnectionRefused,
        abi::ERR_RESET => io::ErrorKind::ConnectionReset,
        abi::ERR_PIPE => io::ErrorKind::BrokenPipe,
        abi::ERR_ACCESS => io::ErrorKind::PermissionDenied,
        abi::ERR_INVALID => io::ErrorKind::InvalidInput,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, e)
}
fn finish(op: Arc<Operation>) -> Result<native_io::Completion, io::Error> {
    let op = Arc::try_unwrap(op)
        .unwrap_or_else(|_| panic!("poll retained a completed native operation"));
    let c = op.finish().map_err(io_error)?;
    if c.error.kind != 0 {
        return Err(io_error(native_io::Error(c.error)));
    }
    Ok(c)
}
impl Connection {
    pub fn new(socket: Arc<Socket>) -> Arc<Self> {
        Arc::new(Self {
            socket,
            read: Mutex::new(Read::Idle),
            write: Mutex::new(Write::Ready),
        })
    }
    pub fn prepare_read(&self, io: Client) -> wasmtime::Result<()> {
        let mut state = self.read.lock().unwrap();
        if matches!(*state, Read::Idle) {
            *state = match io.submit(Some(self.socket.clone()), abi::READ, &[], 65536, None, 0) {
                Ok(op) => Read::Pending(Arc::new(op)),
                Err(e) => Read::Failed(io_error(e)),
            };
        }
        if let Read::Pending(op) = &*state {
            io.poll(&[op], false)?;
            if !op.ready() {
                return Ok(());
            }
        } else {
            return Ok(());
        }
        let Read::Pending(op) = std::mem::replace(&mut *state, Read::Closed) else {
            unreachable!()
        };
        *state = match finish(op) {
            Err(e) => Read::Failed(e),
            Ok(c) if c.kind == abi::EOF || c.bytes.is_empty() => Read::Closed,
            Ok(c) => Read::Bytes {
                data: c.bytes,
                offset: 0,
            },
        };
        Ok(())
    }
    pub fn read(&self, io: Client, len: usize, blocking: bool) -> Result<Vec<u8>, StreamError> {
        if len == 0 {
            return if matches!(*self.read.lock().unwrap(), Read::Closed) {
                Err(StreamError::Closed)
            } else {
                Ok(Vec::new())
            };
        }
        loop {
            self.prepare_read(io).map_err(StreamError::Trap)?;
            let mut state = self.read.lock().unwrap();
            match &mut *state {
                Read::Pending(op) if blocking => {
                    io.poll(&[op], true).map_err(StreamError::Trap)?;
                }
                Read::Pending(_) => return Ok(Vec::new()),
                Read::Bytes { data, offset } => {
                    let n = len.min(data.len() - *offset);
                    let result = data[*offset..*offset + n].to_vec();
                    *offset += n;
                    if *offset == data.len() {
                        *state = Read::Idle;
                    }
                    return Ok(result);
                }
                Read::Failed(_) => {
                    let Read::Failed(e) = std::mem::replace(&mut *state, Read::Closed) else {
                        unreachable!()
                    };
                    return Err(StreamError::Failed(e));
                }
                Read::Closed => return Err(StreamError::Closed),
                Read::Idle => unreachable!(),
            }
        }
    }
    pub fn prepare_write(&self, io: Client) -> wasmtime::Result<()> {
        let mut state = self.write.lock().unwrap();
        if let Write::Pending(op) = &*state {
            io.poll(&[op], false)?;
            if !op.ready() {
                return Ok(());
            }
        } else {
            return Ok(());
        }
        let Write::Pending(op) = std::mem::replace(&mut *state, Write::Closed) else {
            unreachable!()
        };
        *state = match finish(op) {
            Ok(_) => Write::Ready,
            Err(e) => Write::Failed(e),
        };
        Ok(())
    }
    pub fn check_write(&self, io: Client) -> Result<u64, StreamError> {
        self.prepare_write(io).map_err(StreamError::Trap)?;
        let mut state = self.write.lock().unwrap();
        match &*state {
            Write::Pending(_) => Ok(0),
            Write::Ready => Ok(65536),
            Write::Closed => Err(StreamError::Closed),
            Write::Failed(_) => {
                let Write::Failed(e) = std::mem::replace(&mut *state, Write::Closed) else {
                    unreachable!()
                };
                Err(StreamError::Failed(e))
            }
        }
    }
    pub fn write(&self, io: Client, data: Vec<u8>) -> Result<(), StreamError> {
        self.prepare_write(io).map_err(StreamError::Trap)?;
        let mut state = self.write.lock().unwrap();
        match &*state {
            Write::Pending(_) => {
                return Err(StreamError::Trap(wasmtime::format_err!(
                    "write without available permit"
                )))
            }
            Write::Closed => return Err(StreamError::Closed),
            Write::Failed(_) => {
                let Write::Failed(e) = std::mem::replace(&mut *state, Write::Closed) else {
                    unreachable!()
                };
                return Err(StreamError::Failed(e));
            }
            Write::Ready => {}
        }
        if data.is_empty() {
            return Ok(());
        }
        *state = match io.submit(
            Some(self.socket.clone()),
            abi::WRITE,
            &data,
            data.len(),
            None,
            0,
        ) {
            Ok(op) => Write::Pending(Arc::new(op)),
            Err(e) => Write::Failed(io_error(e)),
        };
        Ok(())
    }
    pub fn wait_write(&self, io: Client) -> Result<(), StreamError> {
        loop {
            self.prepare_write(io).map_err(StreamError::Trap)?;
            let op = match &*self.write.lock().unwrap() {
                Write::Pending(op) => Some(op.clone()),
                _ => None,
            };
            if let Some(op) = op {
                io.poll(&[&op], true).map_err(StreamError::Trap)?;
            } else {
                return self.check_write(io).map(|_| ());
            }
        }
    }
    pub fn close_read(&self) {
        *self.read.lock().unwrap() = Read::Closed;
        let _ = self.socket.option(abi::SHUTDOWN_READ, true, [0; 4]);
    }
    pub fn close_write(&self, io: Client) {
        *self.write.lock().unwrap() = Write::Closed;
        if let Ok(op) = io.submit(Some(self.socket.clone()), abi::SHUTDOWN, &[], 0, None, 0) {
            while !op.ready() {
                if io.poll(&[&op], true).is_err() {
                    return;
                }
            }
            let _ = op.finish();
        }
    }
    pub fn read_operation(&self) -> Option<Arc<Operation>> {
        match &*self.read.lock().unwrap() {
            Read::Pending(op) => Some(op.clone()),
            _ => None,
        }
    }
    pub fn write_operation(&self) -> Option<Arc<Operation>> {
        match &*self.write.lock().unwrap() {
            Write::Pending(op) => Some(op.clone()),
            _ => None,
        }
    }
}
