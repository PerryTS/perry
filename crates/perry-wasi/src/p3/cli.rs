//! P3 CLI capabilities reuse the context's explicit P2 native stream views.
use super::streams;
#[cfg(test)]
mod tests;
use super::bindings::cli::{
    environment, exit, stderr, stdin, stdout, terminal_input, terminal_output, terminal_stderr,
    terminal_stdin, terminal_stdout, types,
};
use crate::cli::{WasiCli, WasiCliCtxView};
use futures_channel::oneshot;
use types::ErrorCode;
use wasmtime::component::{Access, FutureReader, Resource, StreamReader};

pub struct TerminalInput;
pub struct TerminalOutput;
impl types::Host for WasiCliCtxView<'_> {}
impl environment::Host for WasiCliCtxView<'_> {
    fn get_environment(&mut self) -> wasmtime::Result<Vec<(String, String)>> {
        Ok(self.ctx.environment.clone())
    }
    fn get_arguments(&mut self) -> wasmtime::Result<Vec<String>> {
        Ok(self.ctx.arguments.clone())
    }
    fn get_initial_cwd(&mut self) -> wasmtime::Result<Option<String>> {
        Ok(self.ctx.initial_cwd.clone())
    }
}
impl exit::Host for WasiCliCtxView<'_> {
    fn exit(&mut self, status: Result<(), ()>) -> wasmtime::Result<()> {
        Err(wasmtime::format_err!(crate::I32Exit(i32::from(
            status.is_err()
        ))))
    }
    fn exit_with_code(&mut self, status: u8) -> wasmtime::Result<()> {
        Err(wasmtime::format_err!(crate::I32Exit(i32::from(status))))
    }
}
impl terminal_input::Host for WasiCliCtxView<'_> {}
impl terminal_output::Host for WasiCliCtxView<'_> {}
impl terminal_input::HostTerminalInput for WasiCliCtxView<'_> {
    fn drop(&mut self, value: Resource<TerminalInput>) -> wasmtime::Result<()> {
        self.table.delete(value)?;
        Ok(())
    }
}
impl terminal_output::HostTerminalOutput for WasiCliCtxView<'_> {
    fn drop(&mut self, value: Resource<TerminalOutput>) -> wasmtime::Result<()> {
        self.table.delete(value)?;
        Ok(())
    }
}
impl terminal_stdin::Host for WasiCliCtxView<'_> {
    fn get_terminal_stdin(&mut self) -> wasmtime::Result<Option<Resource<TerminalInput>>> {
        if self.ctx.stdin.is_terminal() {
            Ok(Some(self.table.push(TerminalInput)?))
        } else {
            Ok(None)
        }
    }
}
impl terminal_stdout::Host for WasiCliCtxView<'_> {
    fn get_terminal_stdout(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        if self.ctx.stdout.is_terminal() {
            Ok(Some(self.table.push(TerminalOutput)?))
        } else {
            Ok(None)
        }
    }
}
impl terminal_stderr::Host for WasiCliCtxView<'_> {
    fn get_terminal_stderr(&mut self) -> wasmtime::Result<Option<Resource<TerminalOutput>>> {
        if self.ctx.stderr.is_terminal() {
            Ok(Some(self.table.push(TerminalOutput)?))
        } else {
            Ok(None)
        }
    }
}
impl stdin::Host for WasiCliCtxView<'_> {}
impl stdout::Host for WasiCliCtxView<'_> {}
impl stderr::Host for WasiCliCtxView<'_> {}

impl<T> stdin::HostWithStore<T> for WasiCli {
    fn read_via_stream(
        mut store: Access<'_, T, Self>,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<Result<(), ErrorCode>>)> {
        let (send, receive) = oneshot::channel();
        let stream = store.get().ctx.stdin.p2_stream();
        let data = StreamReader::new(&mut store, streams::Input::new(stream, send))?;
        let result = FutureReader::new(&mut store, async move {
            wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
        })?;
        Ok((data, result))
    }
}
impl<T> stdout::HostWithStore<T> for WasiCli {
    fn write_via_stream(
        mut store: Access<'_, T, Self>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
        let (send, receive) = oneshot::channel();
        let stream = store.get().ctx.stdout.p2_stream();
        data.pipe(&mut store, streams::Output::new(stream, send))?;
        FutureReader::new(&mut store, async move {
            wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
        })
    }
}
impl<T> stderr::HostWithStore<T> for WasiCli {
    fn write_via_stream(
        mut store: Access<'_, T, Self>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<FutureReader<Result<(), ErrorCode>>> {
        let (send, receive) = oneshot::channel();
        let stream = store.get().ctx.stderr.p2_stream();
        data.pipe(&mut store, streams::Output::new(stream, send))?;
        FutureReader::new(&mut store, async move {
            wasmtime::error::Ok(receive.await.unwrap_or(Ok(())))
        })
    }
}

impl streams::StreamFailure for ErrorCode {
    fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::BrokenPipe => Self::Pipe,
            std::io::ErrorKind::InvalidData => Self::IllegalByteSequence,
            _ => Self::Io,
        }
    }
    fn closed() -> Self {
        Self::Pipe
    }
}
