//! Native WASI 0.3 providers. Clock waits use the component's Turnloop driver.
//! Filesystem, CLI and socket streams retain owned native Turnloop views.
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
            "wasi:sockets/types.[method]tcp-socket.bind": async | trappable,
            "wasi:sockets/types.[method]tcp-socket.listen": async | store | trappable,
            "wasi:sockets/types.[method]tcp-socket.send": store | trappable,
            "wasi:sockets/types.[method]tcp-socket.receive": store | trappable,
            "wasi:sockets/types.[method]udp-socket.bind": async | trappable,
            "wasi:sockets/types.[method]udp-socket.connect": async | trappable,
            default: trappable,
        },
        trappable_error_type: { "wasi:filesystem/types.error-code" => crate::p3::filesystem::FilesystemError, "wasi:sockets/types.error-code" => crate::p3::sockets::SocketError },
        with: {
            "wasi:sockets/types.tcp-socket": crate::p3::sockets::tcp::TcpSocket,
            "wasi:sockets/types.udp-socket": crate::p3::sockets::udp::UdpSocket,
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
pub mod http;
mod random;
mod sockets;
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
    bindings::sockets::types::add_to_linker::<T, crate::sockets::WasiSockets>(linker, |state| {
        let view = state.ctx();
        crate::sockets::WasiSocketsCtxView {
            ctx: &mut view.ctx.sockets,
            table: view.table,
        }
    })?;
    bindings::sockets::ip_name_lookup::add_to_linker::<T, crate::sockets::WasiSockets>(
        linker,
        |state| {
            let view = state.ctx();
            crate::sockets::WasiSocketsCtxView {
                ctx: &mut view.ctx.sockets,
                table: view.table,
            }
        },
    )?;
    http::add_to_linker(linker)
}
