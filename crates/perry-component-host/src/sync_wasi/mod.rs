//! Synchronous Preview 2 providers. Each store owns its capabilities and resources.
use crate::WasiOptions;
use wasmtime::component::ResourceTable;

mod cli;
mod clocks;
mod errors;
mod filesystem;
pub(crate) mod native_io;
pub(crate) mod network;
mod random;
pub(crate) mod sockets;
pub(crate) mod streams;
pub(crate) mod tcp;
mod tcp_io;
pub(crate) mod udp;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "wasi:cli/command",
        imports: { default: trappable },
        trappable_error_type: {
            "wasi:sockets/network.error-code" => super::network::SocketError,
            "wasi:filesystem/types.error-code" => super::errors::FsError,
            "wasi:io/streams.stream-error" => super::streams::StreamError,
        },
        with: {
            "wasi:sockets/tcp.tcp-socket": super::tcp::TcpSocket,
            "wasi:sockets/udp.udp-socket": super::udp::UdpSocket,
            "wasi:sockets/udp.incoming-datagram-stream": super::udp::Incoming,
            "wasi:sockets/udp.outgoing-datagram-stream": super::udp::Outgoing,
            "wasi:sockets/network.network": super::network::Network,
            "wasi:sockets/ip-name-lookup.resolve-address-stream": super::network::ResolveStream,
            "wasi:io/error.error": super::errors::IoError,
            "wasi:io/poll.pollable": super::streams::Pollable,
            "wasi:io/streams.input-stream": super::streams::Input,
            "wasi:io/streams.output-stream": super::streams::Output,
            "wasi:filesystem/types.descriptor": super::filesystem::Descriptor,
            "wasi:filesystem/types.directory-entry-stream": super::filesystem::DirectoryEntries,
            "wasi:cli/terminal-input.terminal-input": super::TerminalInput,
            "wasi:cli/terminal-output.terminal-output": super::TerminalOutput,
        },
        require_store_data_send: false,
    });
}

pub struct TerminalInput;
pub struct TerminalOutput;

/// An exit request terminates the guest call, never the embedding process.
#[derive(Debug)]
pub(crate) struct Exit(pub i32);

impl std::fmt::Display for Exit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WASI exit with status {}", self.0)
    }
}
impl std::error::Error for Exit {}

pub struct State {
    options: WasiOptions,
    initial: std::time::Instant,
    io: native_io::Client,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
    table: ResourceTable,
    insecure_seed: (u64, u64),
    preopens: Vec<(std::sync::Arc<cap_std::fs::Dir>, String, bool)>,
}

impl State {
    pub fn new(options: &WasiOptions) -> wasmtime::Result<Self> {
        wasmtime::ensure!(
            !(options.inherit_stderr && options.stderr_buffer.is_some()),
            "inherit_stderr and stderr_buffer are mutually exclusive"
        );
        let mut seed = [0; 16];
        random::fill(&mut seed)?;
        Ok(Self {
            options: options.clone(),
            initial: std::time::Instant::now(),
            io: native_io::Client::current(),
            _thread: std::marker::PhantomData,
            preopens: filesystem::open_preopens(options)?,
            table: ResourceTable::new(),
            insecure_seed: (
                u64::from_le_bytes(seed[..8].try_into().unwrap()),
                u64::from_le_bytes(seed[8..].try_into().unwrap()),
            ),
        })
    }
}

/// Install the same 27 stable Preview 2 interfaces used by the component host.
/// Each store supplies its own capability/resource state; the embedding thread
/// supplies the existing native driver through the runtime operation boundary.
pub fn add_to_linker<T: 'static>(
    linker: &mut wasmtime::component::Linker<T>,
    state: fn(&mut T) -> &mut State,
) -> wasmtime::Result<()> {
    use bindings::wasi::{cli, clocks, filesystem, io, random, sockets};
    use wasmtime::component::HasSelf;
    cli::environment::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::exit::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::stdin::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::stdout::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::stderr::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::terminal_input::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::terminal_output::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::terminal_stdin::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::terminal_stdout::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    cli::terminal_stderr::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    clocks::monotonic_clock::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    clocks::wall_clock::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    filesystem::types::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    filesystem::preopens::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    io::error::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    io::poll::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    io::streams::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    random::random::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    random::insecure::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    random::insecure_seed::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::instance_network::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::tcp::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::tcp_create_socket::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::udp::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::udp_create_socket::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::ip_name_lookup::add_to_linker::<T, HasSelf<State>>(linker, state)?;
    sockets::network::add_to_linker::<T, HasSelf<State>>(linker, &Default::default(), state)?;
    Ok(())
}
