//! A reference WASI preview 1 host for `wasm32-wasip1` programs.
//!
//! ```text
//! perry-p1-host [--env K=V]... [--listen [FD=]IP:PORT]... [--no-announce]
//!               [--signal MS:BYTE]... [--dns NAME=RESULT[,delay=MS]]...
//!               [--no-discard] [--stats FILE] MODULE [ARG]...
//! ```
//!
//! `--listen` binds a TCP listener in the host and hands it to the guest as
//! the next free descriptor (or as `FD`, leaving a gap below it), named in
//! `PERRY_LISTEN_FDS` and `PERRY_LISTEN_ADDRS`; `--no-announce` leaves those
//! out, so the guest has to find its listeners by scanning. `--signal` queues one byte on the guest's signal
//! stream (`PERRY_SIGNAL_FD`) after the given delay; the stream is closed after
//! the last one. `--stats` writes what the host observed as JSON when the guest
//! exits.
//!
//! `--no-discard` makes `perry_host.discard_pages` answer 0 ("did nothing"),
//! which a guest takes as a host without it. Otherwise the call zero-fills the
//! range, dropping its pages where the OS allows, and `--stats` counts the
//! calls and the bytes.
//!
//! `--dns` sets what the `perry_net` imports resolve a name to: `notfound`,
//! `temp`, or addresses joined by `+` (`127.0.0.2+127.0.0.1` is tried in that
//! order), after an optional delay. A literal address is itself, `localhost` is
//! 127.0.0.1, and any other name is not found: the host never asks the network.

mod discard;
mod fdtable;
mod perry_net;
mod preview1;

use fdtable::{Fd, Table};
use perry_net::Dns;
use preview1::{ExitCode, Host};
use std::net::TcpListener;
use std::os::fd::{FromRawFd, OwnedFd};
use std::time::Duration;
use wasmtime::{Config, Engine, Linker, Module, Store};

struct Options {
    module: String,
    args: Vec<String>,
    env: Vec<String>,
    listen: Vec<String>,
    announce: bool,
    signals: Vec<(u64, u8)>,
    dns: Dns,
    discard: bool,
    stats: Option<String>,
}

fn parse(mut argv: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut opts = Options {
        module: String::new(),
        args: vec![],
        env: vec![],
        listen: vec![],
        announce: true,
        signals: vec![],
        dns: Dns::default(),
        discard: true,
        stats: None,
    };
    while let Some(arg) = argv.next() {
        let mut value = |name: &str| argv.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--env" => opts.env.push(value("--env")?),
            "--listen" => opts.listen.push(value("--listen")?),
            "--no-announce" => opts.announce = false,
            "--no-discard" => opts.discard = false,
            "--dns" => opts.dns.add(&value("--dns")?)?,
            "--stats" => opts.stats = Some(value("--stats")?),
            "--signal" => {
                let v = value("--signal")?;
                let (ms, byte) = v.split_once(':').ok_or("--signal wants MS:BYTE")?;
                opts.signals.push((
                    ms.parse().map_err(|_| "bad --signal delay")?,
                    byte.parse().map_err(|_| "bad --signal byte")?,
                ));
            }
            _ if arg.starts_with("--") => return Err(format!("unknown option {arg}")),
            _ => {
                opts.module = arg;
                opts.args = argv.collect();
                return Ok(opts);
            }
        }
    }
    Err("missing module".into())
}

/// Write the stream's bytes on schedule, then close it.
fn feed_signals(mut schedule: Vec<(u64, u8)>, write_end: OwnedFd) {
    schedule.sort();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        for (ms, byte) in schedule {
            std::thread::sleep(Duration::from_millis(ms).saturating_sub(start.elapsed()));
            unsafe {
                libc::write(
                    std::os::fd::AsRawFd::as_raw_fd(&write_end),
                    (&byte as *const u8).cast(),
                    1,
                )
            };
        }
    });
}

fn run(opts: Options) -> Result<i32, Box<dyn std::error::Error>> {
    let mut table = Table::with_stdio();
    let mut env = opts.env;
    let (mut fds, mut addrs) = (vec![], vec![]);
    for spec in &opts.listen {
        let (at, bind) = match spec.split_once('=') {
            Some((at, bind)) => (Some(at.parse::<u32>()?), bind),
            None => (None, spec.as_str()),
        };
        let listener = TcpListener::bind(bind)?;
        let addr = listener.local_addr()?;
        eprintln!("listening {addr}");
        let entry = Fd::Listener(listener.into());
        let number = match at {
            Some(number) => {
                if !table.insert_at(number, entry) {
                    return Err(format!("descriptor {number} is taken").into());
                }
                number
            }
            None => table.insert(entry),
        };
        fds.push(number.to_string());
        addrs.push(addr.to_string());
    }
    if opts.announce && !fds.is_empty() {
        env.push(format!("PERRY_LISTEN_FDS={}", fds.join(",")));
        env.push(format!("PERRY_LISTEN_ADDRS={}", addrs.join(",")));
    }
    let mut signal_feed = None;
    if !opts.signals.is_empty() {
        let mut ends = [0i32; 2];
        if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let (read_end, write_end) =
            unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
        env.push(format!(
            "PERRY_SIGNAL_FD={}",
            table.insert(Fd::Signal(read_end))
        ));
        signal_feed = Some((opts.signals, write_end));
    }

    let mut config = Config::new();
    config.wasm_exceptions(true);
    // Compile time dominates a short run of a large module.
    config.cranelift_opt_level(wasmtime::OptLevel::None);
    let engine = Engine::new(&config)?;
    let module = Module::from_file(&engine, &opts.module)?;
    let mut linker = Linker::new(&engine);
    preview1::add(&mut linker)?;
    preview1::add_perry_net(&mut linker)?;
    preview1::add_perry_host(&mut linker)?;
    // Anything else a module imports traps when called.
    linker.define_unknown_imports_as_traps(&module)?;

    let mut args = vec![opts.module.clone()];
    args.extend(opts.args);
    let mut host = Host::new(args, env, table, opts.dns);
    host.discard_answers = opts.discard;
    let mut store = Store::new(&engine, host);
    let instance = linker.instantiate(&mut store, &module)?;
    let start = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
    // The delays count from the moment the guest starts, not from compilation.
    if let Some((schedule, write_end)) = signal_feed {
        feed_signals(schedule, write_end);
    }
    let status = match start.call(&mut store, ()) {
        Ok(()) => 0,
        Err(error) => match error.downcast_ref::<ExitCode>() {
            Some(ExitCode(status)) => *status,
            None => {
                eprintln!("p1-host: guest trapped: {error:?}");
                134
            }
        },
    };
    if let Some(path) = opts.stats {
        let s = &store.data().stats;
        let accepts: Vec<String> = s.accepts.iter().map(u32::to_string).collect();
        let connects: Vec<String> = s.connects.iter().map(u32::to_string).collect();
        std::fs::write(path, format!(
            "{{\"polls\":{},\"clock_wakeups\":{},\"fd_wakeups\":{},\"max_subs\":{},\"prestat_calls\":{},\"accepts\":[{}],\"connect_attempts\":{},\"connects\":[{}],\"sock_errors\":{},\"discard_calls\":{},\"discard_bytes\":{}}}\n",
            s.polls, s.clock_wakeups, s.fd_wakeups, s.max_subs, s.prestat_calls, accepts.join(","), s.connect_attempts, connects.join(","), s.sock_errors, s.discard_calls, s.discard_bytes))?;
    }
    Ok(status)
}

fn main() {
    let opts = match parse(std::env::args().skip(1)) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("p1-host: {message}");
            std::process::exit(2);
        }
    };
    match run(opts) {
        Ok(status) => std::process::exit(status),
        Err(error) => {
            eprintln!("p1-host: {error}");
            std::process::exit(2);
        }
    }
}
