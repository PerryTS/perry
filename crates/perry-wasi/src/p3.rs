//! Native WASI 0.3 providers. Clock waits use the component's Turnloop driver.
//! CLI and filesystem streams reuse native Turnloop views; sockets and HTTP remain.
pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit-p3",
        world: "providers",
        require_store_data_send: true,
        imports: {
            "wasi:cli/stdin": store | trappable,
            "wasi:cli/stdout": store | trappable,
            "wasi:cli/stderr": store | trappable,
            "wasi:filesystem/types.[method]descriptor.read-via-stream": store | trappable,
            "wasi:filesystem/types.[method]descriptor.write-via-stream": store | trappable,
            "wasi:filesystem/types.[method]descriptor.append-via-stream": store | trappable,
            "wasi:filesystem/types.[method]descriptor.read-directory": store | trappable,
            default: trappable,
        },
        trappable_error_type: { "wasi:filesystem/types.error-code" => crate::p3::filesystem::FilesystemError },
        with: {
            "wasi:filesystem/types.descriptor": crate::filesystem::Descriptor,
            "wasi:cli/terminal-input.terminal-input": crate::p3::cli::TerminalInput,
            "wasi:cli/terminal-output.terminal-output": crate::p3::cli::TerminalOutput,
        },
    });
    pub use wasi::*;
}

#[cfg(test)]
mod canonical_tests;
mod cli;
mod filesystem;
mod random;
mod streams;
#[cfg(test)]
mod tests;

use crate::{
    clocks::{WasiClocks, WasiClocksCtxView},
    random::WasiRandom,
    WasiView,
};
use wasmtime::component::Linker;

pub fn add_to_linker<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    bindings::clocks::types::add_to_linker::<T, WasiClocks>(linker, |state| {
        let view = state.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::clocks::monotonic_clock::add_to_linker::<T, WasiClocks>(linker, |state| {
        let view = state.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::clocks::system_clock::add_to_linker::<T, WasiClocks>(linker, |state| {
        let view = state.ctx();
        WasiClocksCtxView {
            ctx: &mut view.ctx.clocks,
            table: view.table,
        }
    })?;
    bindings::random::random::add_to_linker::<T, WasiRandom>(linker, |state| {
        &mut state.ctx().ctx.random
    })?;
    bindings::random::insecure::add_to_linker::<T, WasiRandom>(linker, |state| {
        &mut state.ctx().ctx.random
    })?;
    bindings::random::insecure_seed::add_to_linker::<T, WasiRandom>(linker, |state| {
        &mut state.ctx().ctx.random
    })?;
    macro_rules! cli_interface {
        ($($interface:ident),* $(,)?) => {$ (
            bindings::cli::$interface::add_to_linker::<T, crate::cli::WasiCli>(linker, |state| {
                let view = state.ctx();
                crate::cli::WasiCliCtxView { ctx: &mut view.ctx.cli, table: view.table }
            })?;
        )*};
    }
    cli_interface!(
        environment,
        exit,
        types,
        stdin,
        stdout,
        stderr,
        terminal_input,
        terminal_output,
        terminal_stdin,
        terminal_stdout,
        terminal_stderr
    );
    bindings::filesystem::types::add_to_linker::<T, crate::filesystem::WasiFilesystem>(
        linker,
        |state| {
            let view = state.ctx();
            crate::filesystem::WasiFilesystemCtxView {
                ctx: &mut view.ctx.filesystem,
                table: view.table,
            }
        },
    )?;
    bindings::filesystem::preopens::add_to_linker::<T, crate::filesystem::WasiFilesystem>(
        linker,
        |state| {
            let view = state.ctx();
            crate::filesystem::WasiFilesystemCtxView {
                ctx: &mut view.ctx.filesystem,
                table: view.table,
            }
        },
    )?;
    Ok(())
}
