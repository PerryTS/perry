// Adapted from Wasmtime 48.0.5 (Bytecode Alliance contributors).
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

use crate::io::bindings::wasi::io::streams::{
    self as async_streams, Host as AsyncHost, HostInputStream as AsyncHostInputStream,
    HostOutputStream as AsyncHostOutputStream,
};
use crate::io::sync::wasi::io::{
    poll::Pollable,
    streams::{self, InputStream, OutputStream},
};
use futures_lite::future::block_on;
use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi_io::streams::{StreamError, StreamResult};

impl From<async_streams::StreamError> for streams::StreamError {
    fn from(other: async_streams::StreamError) -> Self {
        match other {
            async_streams::StreamError::LastOperationFailed(e) => Self::LastOperationFailed(e),
            async_streams::StreamError::Closed => Self::Closed,
        }
    }
}

impl streams::Host for ResourceTable {
    fn convert_stream_error(&mut self, err: StreamError) -> wasmtime::Result<streams::StreamError> {
        Ok(AsyncHost::convert_stream_error(self, err)?.into())
    }
}

impl streams::HostOutputStream for ResourceTable {
    fn drop(&mut self, stream: Resource<OutputStream>) -> wasmtime::Result<()> {
        block_on(async { AsyncHostOutputStream::drop(self, stream).await })
    }

    fn check_write(&mut self, stream: Resource<OutputStream>) -> StreamResult<u64> {
        AsyncHostOutputStream::check_write(self, stream)
    }

    fn write(&mut self, stream: Resource<OutputStream>, bytes: Vec<u8>) -> StreamResult<()> {
        AsyncHostOutputStream::write(self, stream, bytes)
    }

    fn blocking_write_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        bytes: Vec<u8>,
    ) -> StreamResult<()> {
        block_on(async {
            AsyncHostOutputStream::blocking_write_and_flush(self, stream, bytes).await
        })
    }

    fn blocking_write_zeroes_and_flush(
        &mut self,
        stream: Resource<OutputStream>,
        len: u64,
    ) -> StreamResult<()> {
        block_on(async {
            AsyncHostOutputStream::blocking_write_zeroes_and_flush(self, stream, len).await
        })
    }

    fn subscribe(
        &mut self,
        stream: Resource<OutputStream>,
    ) -> wasmtime::Result<Resource<Pollable>> {
        AsyncHostOutputStream::subscribe(self, stream)
    }

    fn write_zeroes(&mut self, stream: Resource<OutputStream>, len: u64) -> StreamResult<()> {
        AsyncHostOutputStream::write_zeroes(self, stream, len)
    }

    fn flush(&mut self, stream: Resource<OutputStream>) -> StreamResult<()> {
        AsyncHostOutputStream::flush(self, Resource::new_borrow(stream.rep()))
    }

    fn blocking_flush(&mut self, stream: Resource<OutputStream>) -> StreamResult<()> {
        block_on(async {
            AsyncHostOutputStream::blocking_flush(self, Resource::new_borrow(stream.rep())).await
        })
    }

    fn splice(
        &mut self,
        dst: Resource<OutputStream>,
        src: Resource<InputStream>,
        len: u64,
    ) -> StreamResult<u64> {
        AsyncHostOutputStream::splice(self, dst, src, len)
    }

    fn blocking_splice(
        &mut self,
        dst: Resource<OutputStream>,
        src: Resource<InputStream>,
        len: u64,
    ) -> StreamResult<u64> {
        block_on(async { AsyncHostOutputStream::blocking_splice(self, dst, src, len).await })
    }
}

impl streams::HostInputStream for ResourceTable {
    fn drop(&mut self, stream: Resource<InputStream>) -> wasmtime::Result<()> {
        block_on(async { AsyncHostInputStream::drop(self, stream).await })
    }

    fn read(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<Vec<u8>> {
        AsyncHostInputStream::read(self, stream, len)
    }

    fn blocking_read(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<Vec<u8>> {
        block_on(async { AsyncHostInputStream::blocking_read(self, stream, len).await })
    }

    fn skip(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<u64> {
        AsyncHostInputStream::skip(self, stream, len)
    }

    fn blocking_skip(&mut self, stream: Resource<InputStream>, len: u64) -> StreamResult<u64> {
        block_on(async { AsyncHostInputStream::blocking_skip(self, stream, len).await })
    }

    fn subscribe(&mut self, stream: Resource<InputStream>) -> wasmtime::Result<Resource<Pollable>> {
        AsyncHostInputStream::subscribe(self, stream)
    }
}
