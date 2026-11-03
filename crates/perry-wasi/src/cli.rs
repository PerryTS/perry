//! CLI capabilities, with closed/discarded stdio unless explicitly configured.
mod native;
use crate::pipe::{ClosedInputStream, MemoryInputPipe, MemoryOutputPipe, SinkOutputStream};
pub use native::{NativeInput, NativeOutput};
use perry_component_io::Submitter;
use wasmtime::component::{HasData, ResourceTable};
use wasmtime_wasi_io::streams::{InputStream, OutputStream};

pub struct WasiCli;
impl HasData for WasiCli {
    type Data<'a> = WasiCliCtxView<'a>;
}
pub struct WasiCliCtxView<'a> {
    pub ctx: &'a mut WasiCliCtx,
    pub table: &'a mut ResourceTable,
}

pub trait IsTerminal {
    fn is_terminal(&self) -> bool;
}
pub trait StdinStream: IsTerminal + Send {
    fn p2_stream(&self) -> Box<dyn InputStream>;
}
pub trait StdoutStream: IsTerminal + Send {
    fn p2_stream(&self) -> Box<dyn OutputStream>;
}

pub struct WasiCliCtx {
    pub environment: Vec<(String, String)>,
    pub arguments: Vec<String>,
    pub initial_cwd: Option<String>,
    pub stdin: Box<dyn StdinStream>,
    pub stdout: Box<dyn StdoutStream>,
    pub stderr: Box<dyn StdoutStream>,
    submitter: Submitter,
}
impl WasiCliCtx {
    pub fn new(submitter: Submitter) -> Self {
        Self {
            environment: vec![],
            arguments: vec![],
            initial_cwd: None,
            stdin: Box::new(ClosedInputStream),
            stdout: Box::new(SinkOutputStream),
            stderr: Box::new(SinkOutputStream),
            submitter,
        }
    }
    pub fn inherit_stdin(&mut self) {
        self.stdin = Box::new(NativeInput::stdin(self.submitter.clone()));
    }
    pub fn inherit_stdout(&mut self) {
        self.stdout = Box::new(NativeOutput::stdout(self.submitter.clone()));
    }
    pub fn inherit_stderr(&mut self) {
        self.stderr = Box::new(NativeOutput::stderr(self.submitter.clone()));
    }
}

macro_rules! memory_stdio {
    ($ty:ty, $tr:ident, $stream:ident) => {
        impl IsTerminal for $ty {
            fn is_terminal(&self) -> bool {
                false
            }
        }
        impl $tr for $ty {
            fn p2_stream(&self) -> Box<dyn $stream> {
                Box::new(self.clone())
            }
        }
    };
}
memory_stdio!(MemoryInputPipe, StdinStream, InputStream);
memory_stdio!(MemoryOutputPipe, StdoutStream, OutputStream);
impl IsTerminal for ClosedInputStream {
    fn is_terminal(&self) -> bool {
        false
    }
}
impl StdinStream for ClosedInputStream {
    fn p2_stream(&self) -> Box<dyn InputStream> {
        Box::new(*self)
    }
}
impl IsTerminal for SinkOutputStream {
    fn is_terminal(&self) -> bool {
        false
    }
}
impl StdoutStream for SinkOutputStream {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(*self)
    }
}

#[cfg(test)]
mod tests;
