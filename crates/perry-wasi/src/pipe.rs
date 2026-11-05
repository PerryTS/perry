// Adapted from wasmtime-wasi 48.0.5 src/p2/pipe.rs.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

use bytes::Bytes;
use std::sync::{Arc, Mutex};
use wasmtime::format_err;
use wasmtime_wasi_io::{
    poll::Pollable,
    streams::{InputStream, OutputStream, StreamError},
};

#[derive(Debug, Clone)]
pub struct MemoryInputPipe {
    buffer: Arc<Mutex<Bytes>>,
}

impl MemoryInputPipe {
    pub fn new(bytes: impl Into<Bytes>) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(bytes.into())),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.lock().unwrap().is_empty()
    }
}

#[async_trait::async_trait]
impl InputStream for MemoryInputPipe {
    fn read(&mut self, size: usize) -> Result<Bytes, StreamError> {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.is_empty() {
            return Err(StreamError::Closed);
        }

        let size = size.min(buffer.len());
        let read = buffer.split_to(size);
        Ok(read)
    }
}

#[async_trait::async_trait]
impl Pollable for MemoryInputPipe {
    async fn ready(&mut self) {}
}

#[derive(Debug, Clone)]
pub struct MemoryOutputPipe {
    capacity: usize,
    buffer: Arc<Mutex<bytes::BytesMut>>,
}

impl MemoryOutputPipe {
    pub fn new(capacity: usize) -> Self {
        MemoryOutputPipe {
            capacity,
            buffer: std::sync::Arc::new(std::sync::Mutex::new(bytes::BytesMut::new())),
        }
    }

    pub fn contents(&self) -> bytes::Bytes {
        self.buffer.lock().unwrap().clone().freeze()
    }

    pub fn try_into_inner(self) -> Option<bytes::BytesMut> {
        std::sync::Arc::into_inner(self.buffer).map(|m| m.into_inner().unwrap())
    }
}

#[async_trait::async_trait]
impl OutputStream for MemoryOutputPipe {
    fn write(&mut self, bytes: Bytes) -> Result<(), StreamError> {
        let mut buf = self.buffer.lock().unwrap();
        if bytes.len() > self.capacity - buf.len() {
            return Err(StreamError::Trap(format_err!(
                "write beyond capacity of MemoryOutputPipe"
            )));
        }
        buf.extend_from_slice(bytes.as_ref());
        // Always ready for writing
        Ok(())
    }
    fn flush(&mut self) -> Result<(), StreamError> {
        // This stream is always flushed
        Ok(())
    }
    fn check_write(&mut self) -> Result<usize, StreamError> {
        let consumed = self.buffer.lock().unwrap().len();
        if consumed < self.capacity {
            Ok(self.capacity - consumed)
        } else {
            // Since the buffer is full, no more bytes will ever be written
            Err(StreamError::Closed)
        }
    }
}

#[async_trait::async_trait]
impl Pollable for MemoryOutputPipe {
    async fn ready(&mut self) {}
}

/// An output stream that consumes all input written to it, and is always ready.
#[derive(Copy, Clone)]
pub struct SinkOutputStream;

#[async_trait::async_trait]
impl OutputStream for SinkOutputStream {
    fn write(&mut self, _buf: Bytes) -> Result<(), StreamError> {
        Ok(())
    }
    fn flush(&mut self) -> Result<(), StreamError> {
        // This stream is always flushed
        Ok(())
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        // This stream is always ready for writing.
        Ok(64 * 1024)
    }
}

#[async_trait::async_trait]
impl Pollable for SinkOutputStream {
    async fn ready(&mut self) {}
}

/// A stream that is ready immediately, but will always report that it's closed.
#[derive(Copy, Clone)]
pub struct ClosedInputStream;

#[async_trait::async_trait]
impl InputStream for ClosedInputStream {
    fn read(&mut self, _size: usize) -> Result<Bytes, StreamError> {
        Err(StreamError::Closed)
    }
}

#[async_trait::async_trait]
impl Pollable for ClosedInputStream {
    async fn ready(&mut self) {}
}

/// An output stream that is always closed.
#[derive(Copy, Clone)]
pub struct ClosedOutputStream;

#[async_trait::async_trait]
impl OutputStream for ClosedOutputStream {
    fn write(&mut self, _: Bytes) -> Result<(), StreamError> {
        Err(StreamError::Closed)
    }
    fn flush(&mut self) -> Result<(), StreamError> {
        Err(StreamError::Closed)
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        Err(StreamError::Closed)
    }
}

#[async_trait::async_trait]
impl Pollable for ClosedOutputStream {
    async fn ready(&mut self) {}
}
