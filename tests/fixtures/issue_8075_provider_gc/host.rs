use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::mpsc::{self, Receiver, SyncSender};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const CHECK_INTERVAL: usize = 256;
const GROWTH_FLOOR: u64 = 1024 * 1024;
const SERIAL_CALLS: usize = 16_384;
const CONCURRENT_CALLS: usize = 16_384;
const EXPECTED_BODY: &[u8] = br#"{"runtime":"perry","iterations":100,"checksum":3726872593}"#;

#[cfg(target_os = "linux")]
const RTLD_GLOBAL: c_int = 0x100;
#[cfg(target_os = "macos")]
const RTLD_GLOBAL: c_int = 0x8;
#[cfg(target_os = "linux")]
const RTLD_LOCAL: c_int = 0;
#[cfg(target_os = "macos")]
const RTLD_LOCAL: c_int = 0x4;
const RTLD_NOW: c_int = 2;

#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

type GcInit = unsafe extern "C" fn();
type ModuleInit = unsafe extern "C" fn();
type Handler = unsafe extern "C" fn(i64, f64) -> f64;
type BufferAlloc = unsafe extern "C" fn(i32, i32) -> *mut c_void;
type BufferData = unsafe extern "C" fn(f64) -> *mut u8;
type BufferLen = unsafe extern "C" fn(f64) -> usize;
type ArenaStats = unsafe extern "C" fn(*mut u64, *mut u64);
type MemoryPressure = unsafe extern "C" fn(u32) -> u32;
type RuntimeProbe = unsafe extern "C" fn() -> usize;

#[derive(Clone, Copy)]
struct RuntimeApi {
    gc_init: GcInit,
    buffer_alloc: BufferAlloc,
    buffer_data: BufferData,
    buffer_len: BufferLen,
    arena_stats: ArenaStats,
    memory_pressure: MemoryPressure,
}

#[derive(Clone, Copy)]
enum BodyKind {
    Temporary,
    Retained,
}

enum Command {
    Invoke {
        kind: BodyKind,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Shutdown(mpsc::Sender<Result<RunStats, String>>),
}

#[derive(Debug)]
struct RunStats {
    calls: usize,
    temporary_calls: usize,
    retained_calls: usize,
    full_collections: usize,
    reclaimed_bytes: u64,
    post_collection_live: Vec<u64>,
}

fn dynamic_error(context: &str) -> String {
    let detail = unsafe {
        let error = dlerror();
        if error.is_null() {
            "unknown loader error".into()
        } else {
            CStr::from_ptr(error).to_string_lossy().into_owned()
        }
    };
    format!("{context}: {detail}")
}

fn open(path: &str, flags: c_int) -> Result<usize, String> {
    let path = CString::new(path).map_err(|_| format!("NUL in library path {path:?}"))?;
    let handle = unsafe { dlopen(path.as_ptr(), flags) };
    if handle.is_null() {
        Err(dynamic_error("dlopen failed"))
    } else {
        Ok(handle as usize)
    }
}

unsafe fn symbol<T: Copy>(handle: usize, name: &str) -> Result<T, String> {
    let name = CString::new(name).map_err(|_| format!("NUL in symbol {name:?}"))?;
    let pointer = dlsym(handle as *mut c_void, name.as_ptr());
    if pointer.is_null() {
        return Err(dynamic_error("dlsym failed"));
    }
    Ok(std::mem::transmute_copy::<*mut c_void, T>(&pointer))
}

fn arena_stats(api: RuntimeApi) -> (u64, u64) {
    let mut live = 0;
    let mut reserved = 0;
    unsafe { (api.arena_stats)(&mut live, &mut reserved) };
    (live, reserved)
}

fn input_buffer(api: RuntimeApi) -> Result<f64, String> {
    const INPUT: &[u8] = b"PCH2\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let header = unsafe { (api.buffer_alloc)(INPUT.len() as i32, 0) };
    if header.is_null() {
        return Err("input Buffer allocation failed".into());
    }
    let value = f64::from_bits(POINTER_TAG | (header as u64 & POINTER_MASK));
    let data = unsafe { (api.buffer_data)(value) };
    if data.is_null() {
        return Err("input Buffer has no data".into());
    }
    unsafe { std::ptr::copy_nonoverlapping(INPUT.as_ptr(), data, INPUT.len()) };
    Ok(value)
}

fn validate_frame(api: RuntimeApi, handler: Handler, invocation: usize) -> Result<(), String> {
    let argument = input_buffer(api)?;
    let result = unsafe { handler(0, argument) };
    let data = unsafe { (api.buffer_data)(result) };
    let length = unsafe { (api.buffer_len)(result) };
    if data.is_null() {
        return Err(format!("invocation {invocation} returned a non-Buffer"));
    }
    let frame = unsafe { std::slice::from_raw_parts(data, length) };
    if frame.len() != 15 + EXPECTED_BODY.len() {
        return Err(format!(
            "invocation {invocation}: frame length {}, expected {}; frame={frame:?}",
            frame.len(),
            15 + EXPECTED_BODY.len()
        ));
    }
    if &frame[..5] != b"PCH2\x02" {
        return Err(format!("invocation {invocation}: invalid PCH2 prefix"));
    }
    if u16::from_be_bytes([frame[5], frame[6]]) != 200 {
        return Err(format!("invocation {invocation}: status is not 200"));
    }
    if u32::from_be_bytes(frame[7..11].try_into().unwrap()) != 0 {
        return Err(format!("invocation {invocation}: headers are not empty"));
    }
    let body_len = u32::from_be_bytes(frame[11..15].try_into().unwrap()) as usize;
    if body_len != EXPECTED_BODY.len() || &frame[15..] != EXPECTED_BODY {
        return Err(format!(
            "invocation {invocation}: corrupt body {:?}",
            String::from_utf8_lossy(&frame[15..])
        ));
    }
    Ok(())
}

fn finish(stats: RunStats) -> Result<RunStats, String> {
    if stats.calls < 20_000 {
        return Err(format!("only {} invocations ran", stats.calls));
    }
    if stats.temporary_calls == 0 || stats.retained_calls == 0 {
        return Err("temporary and retained Buffer variants did not both run".into());
    }
    if stats.full_collections < 10 {
        return Err(format!(
            "only {} host-boundary full collections completed",
            stats.full_collections
        ));
    }
    if stats.reclaimed_bytes < 10 * GROWTH_FLOOR {
        return Err(format!(
            "dead temporary buffers reclaimed only {} bytes",
            stats.reclaimed_bytes
        ));
    }
    let second_half = &stats.post_collection_live[stats.post_collection_live.len() / 2..];
    let first = second_half.first().copied().unwrap_or(0);
    let last = second_half.last().copied().unwrap_or(0);
    if last.saturating_sub(first) > GROWTH_FLOOR / 4 {
        return Err(format!(
            "second-half live arena is not flat: first={first}, last={last}"
        ));
    }
    Ok(stats)
}

fn executor(
    receiver: Receiver<Command>,
    api: RuntimeApi,
    module_init: ModuleInit,
    temporary: Handler,
    retained: Handler,
) {
    unsafe {
        (api.gc_init)();
        module_init();
    }
    let mut stats = RunStats {
        calls: 0,
        temporary_calls: 0,
        retained_calls: 0,
        full_collections: 0,
        reclaimed_bytes: 0,
        post_collection_live: Vec::new(),
    };
    let mut live_baseline = arena_stats(api).0;

    while let Ok(command) = receiver.recv() {
        match command {
            Command::Invoke { kind, reply } => {
                stats.calls += 1;
                let handler = match kind {
                    BodyKind::Temporary => {
                        stats.temporary_calls += 1;
                        temporary
                    }
                    BodyKind::Retained => {
                        stats.retained_calls += 1;
                        retained
                    }
                };
                let result = validate_frame(api, handler, stats.calls);
                if result.is_ok() && stats.calls.is_multiple_of(CHECK_INTERVAL) {
                    let before = arena_stats(api).0;
                    if before.saturating_sub(live_baseline) >= GROWTH_FLOOR {
                        let collected = unsafe { (api.memory_pressure)(2) };
                        if collected == 2 {
                            let after = arena_stats(api).0;
                            stats.full_collections += 1;
                            stats.reclaimed_bytes += before.saturating_sub(after);
                            stats.post_collection_live.push(after);
                            live_baseline = after;
                        }
                    }
                }
                let _ = reply.send(result);
            }
            Command::Shutdown(reply) => {
                let _ = reply.send(finish(stats));
                break;
            }
        }
    }
}

fn invoke(sender: &SyncSender<Command>, kind: BodyKind) -> Result<(), String> {
    let (reply, receive) = mpsc::channel();
    sender
        .send(Command::Invoke { kind, reply })
        .map_err(|_| "executor stopped".to_string())?;
    receive
        .recv()
        .map_err(|_| "executor dropped invocation reply".to_string())?
}

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 6 {
        return Err("usage: host runtime stdlib app temporary-symbol retained-symbol".into());
    }
    let runtime = open(&arguments[1], RTLD_NOW | RTLD_GLOBAL)?;
    let stdlib = open(&arguments[2], RTLD_NOW | RTLD_GLOBAL)?;
    let app = open(&arguments[3], RTLD_NOW | RTLD_LOCAL)?;
    let api = unsafe {
        RuntimeApi {
            gc_init: symbol(runtime, "js_gc_init")?,
            buffer_alloc: symbol(runtime, "js_buffer_alloc")?,
            buffer_data: symbol(runtime, "js_native_buffer_data_ptr")?,
            buffer_len: symbol(runtime, "js_native_buffer_byte_len")?,
            arena_stats: symbol(runtime, "js_arena_stats")?,
            memory_pressure: symbol(runtime, "js_gc_memory_pressure")?,
        }
    };
    let probe: RuntimeProbe = unsafe { symbol(stdlib, "issue_8075_stdlib_runtime_probe")? };
    if unsafe { probe() } != api.gc_init as usize {
        return Err("stdlib provider is bound to a different runtime image".into());
    }
    let module_init: ModuleInit = unsafe { symbol(app, "perry_module_init")? };
    let temporary: Handler = unsafe { symbol(app, &arguments[4])? };
    let retained: Handler = unsafe { symbol(app, &arguments[5])? };

    let (sender, receiver) = mpsc::sync_channel(256);
    let executor_thread =
        std::thread::spawn(move || executor(receiver, api, module_init, temporary, retained));

    for invocation in 0..SERIAL_CALLS {
        let kind = if invocation.is_multiple_of(257) {
            BodyKind::Retained
        } else {
            BodyKind::Temporary
        };
        invoke(&sender, kind)?;
    }

    let mut producers = Vec::new();
    for producer in 0..4 {
        let sender = sender.clone();
        producers.push(std::thread::spawn(move || -> Result<(), String> {
            let calls = CONCURRENT_CALLS / 4;
            for batch in (0..calls).step_by(32) {
                let mut replies = Vec::new();
                for offset in 0..32.min(calls - batch) {
                    let invocation = producer * calls + batch + offset;
                    let kind = if invocation.is_multiple_of(257) {
                        BodyKind::Retained
                    } else {
                        BodyKind::Temporary
                    };
                    let (reply, receive) = mpsc::channel();
                    sender
                        .send(Command::Invoke { kind, reply })
                        .map_err(|_| "executor stopped during concurrent phase".to_string())?;
                    replies.push(receive);
                }
                for reply in replies {
                    reply
                        .recv()
                        .map_err(|_| "executor dropped a concurrent reply".to_string())??;
                }
            }
            Ok(())
        }));
    }
    for producer in producers {
        producer
            .join()
            .map_err(|_| "concurrent producer panicked".to_string())??;
    }

    let (reply, receive) = mpsc::channel();
    sender
        .send(Command::Shutdown(reply))
        .map_err(|_| "executor stopped before shutdown".to_string())?;
    let stats = receive
        .recv()
        .map_err(|_| "executor dropped shutdown report".to_string())??;
    executor_thread
        .join()
        .map_err(|_| "executor panicked".to_string())?;
    println!(
        "issue-8075 provider GC gate passed: calls={} temporary={} retained={} full_collections={} reclaimed_bytes={} second_half_live={:?}",
        stats.calls,
        stats.temporary_calls,
        stats.retained_calls,
        stats.full_collections,
        stats.reclaimed_bytes,
        &stats.post_collection_live[stats.post_collection_live.len() / 2..]
    );
    Ok(())
}
