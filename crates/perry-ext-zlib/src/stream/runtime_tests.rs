//! Codec witnesses through the real runtime Transform, rather than a mock runner.
use super::*;
use std::cell::RefCell;

thread_local! {
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static RELEASE: RefCell<Option<(usize, usize)>> = const { RefCell::new(None) };
    static BOMB: std::cell::Cell<(usize, bool, usize)> = const { std::cell::Cell::new((0, true, 0)) };
    static BOMB_CRC: RefCell<flate2::Crc> = RefCell::new(flate2::Crc::new());
    static BOMB_BOUNDS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
    static ONE_SHOTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ONE_SHOT_ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn one_shot(_: *const RawClosureHeader, _: JsThis, err: f64, output: f64) -> f64 {
    let result = bytes::no_gc(|scope| {
        bytes::borrow(JsValue::from_bits(output.to_bits()), scope).map(|v| v.to_vec())
    });
    let valid = err.to_bits() == JsValue::NULL.bits()
        && result
            .as_ref()
            .is_some_and(|v| crate::gunzip_bytes(v).is_ok_and(|v| v == b"traced one-shot input"));
    if !valid {
        ONE_SHOT_ERRORS.with(|errors| {
            if errors.borrow().len() < 10 {
                errors.borrow_mut().push(format!(
                    "err={:x} output={:x} bytes={:?}",
                    err.to_bits(),
                    output.to_bits(),
                    result.as_ref().map(Vec::len)
                ));
            }
        });
    }
    ONE_SHOTS.with(|n| n.set(n.get() + 1));
    undefined()
}

#[test]
fn ten_thousand_one_shots_keep_captures_through_a_collection_before_run() {
    let _agent = super::OwnAgent::enter();
    clear();
    ONE_SHOTS.with(|n| n.set(0));
    ONE_SHOT_ERRORS.with(|v| v.borrow_mut().clear());
    for index in 0..10000 {
        let roots = TransientRootScope::enter();
        // Exercise both nursery and tracked-large closure storage. A large
        // closure is swept by this full collection even when the current
        // nursery block has not yet reached its copying threshold.
        let captures = if index == 0 {
            vec![undefined(); 2048]
        } else {
            Vec::new()
        };
        let callback = roots.root_nanbox(closure(
            perry_ffi::js_function_info!(one_shot, 2),
            &captures,
        ));
        let input = roots.root_nanbox(value_bytes(b"traced one-shot input"));
        unsafe { queue_one_shot_callback(input.get(), undefined(), callback.get(), Codec::Gzip) };
    }
    assert_eq!(ONE_SHOTS.with(std::cell::Cell::get), 0);
    eprintln!("10k: queued, collecting");
    let mut before = 0;
    perry_runtime::gc::js_gc_stats(&mut before, std::ptr::null_mut(), std::ptr::null_mut());
    perry_runtime::gc::js_gc_collect();
    let mut after = 0;
    perry_runtime::gc::js_gc_stats(&mut after, std::ptr::null_mut(), std::ptr::null_mut());
    assert!(after > before, "the forced collection actually ran");
    // Reuse dead closure-sized storage before dispatch: a raw native
    // callback address must not appear valid merely because reclaimed bytes
    // have not yet been overwritten.
    for _ in 0..20000 {
        let _ = value_bytes(&[0; 64]);
    }
    eprintln!("10k: collected, pumping");
    pump();
    eprintln!("10k: pumped");
    assert!(
        ONE_SHOT_ERRORS.with(|v| v.borrow().is_empty()),
        "{:?}",
        ONE_SHOT_ERRORS.with(|v| v.borrow().clone())
    );
    assert_eq!(ONE_SHOTS.with(std::cell::Cell::get), 10000);
}
extern "C" {
    fn js_nm_install_zlib();
}
fn clear() {
    perry_runtime::gc::gc_init();
    unsafe { js_nm_install_zlib() };
    OUTPUT.with(|v| v.borrow_mut().clear());
    EVENTS.with(|v| v.borrow_mut().clear());
    RELEASE.with(|v| *v.borrow_mut() = None);
}
fn undefined() -> f64 {
    f64::from_bits(UNDEFINED)
}
fn string(s: &str) -> f64 {
    f64::from_bits(JsValue::from_string_ptr(alloc_string(s).as_raw()).bits())
}
fn closure(info: &'static perry_ffi::JsFunctionInfo, captures: &[f64]) -> f64 {
    let roots = TransientRootScope::enter();
    let values: Vec<_> = captures.iter().map(|v| roots.root_nanbox(*v)).collect();
    let result = perry_ffi::alloc_closure(info, values.len() as u32);
    for (i, v) in values.iter().enumerate() {
        unsafe { perry_ffi::set_closure_capture_f64(result, i as u32, v.get()) };
    }
    f64::from_bits(JsValue::from_object_ptr(result).bits())
}
fn pump() {
    for _ in 0..200000 {
        let micro = perry_runtime::promise::js_promise_run_microtasks();
        let immediate = perry_runtime::timer::js_event_loop_check_phase();
        if micro == 0 && immediate == 0 {
            return;
        }
    }
    panic!("runtime did not become idle");
}
fn native_bytes(owner: f64) -> usize {
    let ptr =
        JsValue::from_bits(owner.to_bits()).as_pointer::<perry_runtime::object::ObjectHeader>();
    unsafe {
        let state = (*(*ptr).meta).native_state;
        let cell = JsValue::from_bits(state)
            .as_pointer::<perry_runtime::native_handle::NativeHandleHeader>();
        (*cell).external_bytes as usize
    }
}
extern "C" fn data(c: *const RawClosureHeader, _: JsThis, chunk: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(c, 0) });
    let action = unsafe { perry_ffi::closure_capture_f64(c, 1) };
    let bytes = bytes::no_gc(|scope| {
        bytes::borrow(JsValue::from_bits(chunk.to_bits()), scope)
            .unwrap()
            .to_vec()
    });
    if action == 3.0 {
        let (count, valid, peak) = BOMB.with(std::cell::Cell::get);
        BOMB_CRC.with(|crc| crc.borrow_mut().update(&bytes));
        BOMB_BOUNDS.with(|bounds| {
            let (readable, native) = bounds.get();
            bounds.set((
                readable.max(field(owner.get(), "readableLength") as usize),
                native.max(native_bytes(owner.get())),
            ));
        });
        let current = if count % (128 * 1024) < bytes.len() {
            rss()
        } else {
            peak
        };
        BOMB.with(|stats| {
            stats.set((
                count + bytes.len(),
                valid && bytes.iter().all(|b| *b == 65),
                peak.max(current),
            ))
        });
        if count == 0 {
            unsafe { method(owner.get(), "pause", &[]) };
        }
    } else {
        OUTPUT.with(|v| v.borrow_mut().extend(bytes));
    }
    EVENTS.with(|v| v.borrow_mut().push("data".into()));
    if action == 1.0 {
        unsafe { method(owner.get(), "pause", &[]) };
    }
    if action == 2.0 {
        let before = native_bytes(owner.get());
        unsafe { method(owner.get(), "destroy", &[]) };
        RELEASE.with(|v| *v.borrow_mut() = Some((before, native_bytes(owner.get()))));
    }
    undefined()
}
extern "C" fn named(c: *const RawClosureHeader, _: JsThis) -> f64 {
    let name = unsafe { perry_ffi::closure_capture_f64(c, 0) };
    let name = unsafe { read_input_from_bits(name.to_bits() as i64) }.unwrap();
    EVENTS.with(|v| v.borrow_mut().push(String::from_utf8(name).unwrap()));
    undefined()
}
extern "C" fn errored(_: *const RawClosureHeader, _: JsThis, err: f64) -> f64 {
    assert!(!JsValue::from_bits(err.to_bits()).is_undefined());
    EVENTS.with(|v| v.borrow_mut().push("error".into()));
    undefined()
}
fn listen(owner: f64, action: f64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(owner);
    let callback = roots.root_nanbox(closure(
        perry_ffi::js_function_info!(data, 1),
        &[owner.get(), action],
    ));
    let event = roots.root_nanbox(string("data"));
    unsafe { method(owner.get(), "on", &[event.get(), callback.get()]) };
    for event in ["finish", "end", "close"] {
        let event = roots.root_nanbox(string(event));
        let callback = roots.root_nanbox(closure(
            perry_ffi::js_function_info!(named, 0),
            &[event.get()],
        ));
        unsafe { method(owner.get(), "on", &[event.get(), callback.get()]) };
    }
    let event = roots.root_nanbox(string("error"));
    let callback = roots.root_nanbox(closure(perry_ffi::js_function_info!(errored, 1), &[]));
    unsafe { method(owner.get(), "on", &[event.get(), callback.get()]) };
}
fn factory(name: &str, opts: f64) -> f64 {
    unsafe { crate::js_ext_zlib_native_dispatch(name.as_ptr(), name.len(), &opts, 1) }
}
#[test]
fn a_fresh_factory_installs_its_inherited_methods_without_a_stream_import() {
    let _agent = super::OwnAgent::enter();
    if std::env::var("PERRY_TEST_ZLIB_FRESH_FACTORY").is_err() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "stream::runtime_tests::a_fresh_factory_installs_its_inherited_methods_without_a_stream_import", "--nocapture"])
            .env("PERRY_TEST_ZLIB_FRESH_FACTORY", "1")
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    // Do not call clear(): it installs the module and would hide the bug.
    perry_runtime::gc::gc_init();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    let owner = roots.root_nanbox(factory("Gzip", opts.get()));
    for name in ["on", "end", "destroy", "pipe"] {
        assert!(
            unsafe { js_zlib_is_callback(field(owner.get(), name)) } != 0,
            "inherited {name}"
        );
    }
    listen(owner.get(), 0.0);
    let input = roots.root_nanbox(value_bytes(b"fresh binding factory"));
    unsafe { method(owner.get(), "end", &[input.get()]) };
    pump();
    let output = OUTPUT.with(|bytes| bytes.borrow().clone());
    assert_eq!(
        crate::gunzip_bytes(&output).unwrap(),
        b"fresh binding factory"
    );
    assert_eq!(native_bytes(owner.get()), 0);
}
#[test]
fn constructor_fields_survive_collection_before_the_last_write_state() {
    let _agent = super::OwnAgent::enter();
    if std::env::var("PERRY_TEST_ZLIB_CONSTRUCTOR_GC").is_err() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "stream::runtime_tests::constructor_fields_survive_collection_before_the_last_write_state", "--nocapture"])
            .env("PERRY_TEST_ZLIB_CONSTRUCTOR_GC", "1")
            .output().unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    clear();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    for index in 0..1000 {
        let scope = TransientRootScope::enter();
        let owner = scope.root_nanbox(factory("ZstdDecompress", opts.get()));
        assert_eq!(field(owner.get(), "bytesWritten"), 0.0, "iteration {index}");
        assert!(native_bytes(owner.get()) > 0);
        unsafe { method(owner.get(), "destroy", &[]) };
        pump();
        assert_eq!(native_bytes(owner.get()), 0);
    }
}
// Node 26.5.1's own-key order, independently captured by the eleven-
// constructor oracle. Inherited methods never become instance properties.
fn own_names(owner: f64) -> Vec<String> {
    let roots = TransientRootScope::enter();
    let names = perry_runtime::object::js_object_keys_value(owner);
    let names = roots.root_nanbox(f64::from_bits(JsValue::from_object_ptr(names).bits()));
    let ptr = || {
        JsValue::from_bits(names.get().to_bits()).as_pointer::<perry_runtime::array::ArrayHeader>()
    };
    let count = perry_runtime::array::js_array_length(ptr());
    (0..count)
        .map(|i| {
            let name = perry_runtime::array::js_array_get_f64(ptr(), i);
            String::from_utf8(unsafe { read_input_from_bits(name.to_bits() as i64) }.unwrap())
                .unwrap()
        })
        .collect()
}
fn assert_node_own_names(owner: f64, codec: Codec) {
    let zstd = matches!(codec, Codec::ZstdCompress | Codec::ZstdDecompress);
    let mut expected = vec![
        "_events",
        "_readableState",
        "_writableState",
        "allowHalfOpen",
        "_maxListeners",
        "_eventsCount",
        "bytesWritten",
        "_handle",
        "_outBuffer",
        "_outOffset",
        "_chunkSize",
        "_defaultFlushFlag",
        "_finishFlushFlag",
        "_defaultFullFlushFlag",
        "_flushBoundIdx",
        "_info",
        "_maxOutputLength",
        "_rejectGarbageAfterEnd",
    ];
    if zstd {
        expected.push("_writeState");
    } else {
        expected.insert(0, "_writeState");
        if !matches!(codec, Codec::BrotliCompress | Codec::BrotliDecompress) {
            expected.extend(["_level", "_strategy", "_mode"]);
        }
    }
    assert_eq!(own_names(owner), expected, "Node constructor field order");
}
fn options() -> f64 {
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    np::own(opts.get(), "chunkSize", 1024.0);
    np::own(opts.get(), "readableHighWaterMark", 2048.0);
    opts.get()
}
#[test]
fn eleven_codecs_are_deferred_runtime_transforms_and_release_on_completion() {
    let _agent = super::OwnAgent::enter();
    let input: Vec<_> = (0..100000).map(|i| (i % 251) as u8).collect();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    for (name, codec) in [
        ("Gzip", Codec::Gzip),
        ("Gunzip", Codec::Gunzip),
        ("Deflate", Codec::Deflate),
        ("Inflate", Codec::Inflate),
        ("DeflateRaw", Codec::DeflateRaw),
        ("InflateRaw", Codec::InflateRaw),
        ("Unzip", Codec::Unzip),
        ("BrotliCompress", Codec::BrotliCompress),
        ("BrotliDecompress", Codec::BrotliDecompress),
        ("ZstdCompress", Codec::ZstdCompress),
        ("ZstdDecompress", Codec::ZstdDecompress),
    ] {
        clear();
        let data = match codec {
            Codec::Gunzip | Codec::Unzip => crate::gzip_bytes(&input).unwrap(),
            Codec::Inflate => crate::deflate_bytes(&input).unwrap(),
            Codec::InflateRaw => {
                crate::deflate_raw_bytes_with(&input, Compression::default()).unwrap()
            }
            Codec::BrotliDecompress => brotli_compress_bytes(&input),
            Codec::ZstdDecompress => zstd::stream::encode_all(input.as_slice(), 3).unwrap(),
            _ => input.clone(),
        };
        let owner = roots.root_nanbox(factory(name, opts.get()));
        assert_node_own_names(owner.get(), codec);
        listen(owner.get(), 0.0);
        let chunk = roots.root_nanbox(value_bytes(&data));
        unsafe { method(owner.get(), "end", &[chunk.get()]) };
        assert!(
            OUTPUT.with(|v| v.borrow().is_empty()),
            "data fired inside end: {name}"
        );
        pump();
        let output = OUTPUT.with(|v| v.borrow().clone());
        let output = match codec {
            Codec::Gzip => crate::gunzip_bytes(&output).unwrap(),
            Codec::Deflate => crate::inflate_bytes(&output).unwrap(),
            Codec::DeflateRaw => crate::inflate_raw_bytes(&output).unwrap(),
            Codec::BrotliCompress => brotli_decompress_bytes(&output).unwrap(),
            Codec::ZstdCompress => zstd::stream::decode_all(output.as_slice()).unwrap(),
            _ => output,
        };
        assert_eq!(output, input, "runtime codec {name}");
        let events = EVENTS.with(|v| v.borrow().clone());
        assert!(!events.contains(&"error".into()), "{name}: {events:?}");
        assert_eq!(
            events.iter().filter(|e| *e == "close").count(),
            1,
            "{name}: {events:?}"
        );
        assert_eq!(native_bytes(owner.get()), 0, "autoDestroy releases {name}");
        assert_eq!(
            field(owner.get(), "_handle").to_bits(),
            JsValue::NULL.bits()
        );
    }
}
#[test]
fn real_gunzip_bomb_parks_on_pause_and_keeps_the_input_traced() {
    let _agent = super::OwnAgent::enter();
    clear();
    let _barriers = CompiledBarriers::new();
    let copies_before = perry_runtime::gc::copying_minor_cycles();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    // Generated in a separate Node process. A 100 MB producer allocation
    // retained by the allocator would mask the decompressor's RSS growth.
    let compressed = include_bytes!("../../../../test-files/fixtures/zlib-bomb-100mb.gz");
    let expected = 2229916188;
    let owner = roots.root_nanbox(factory("Gunzip", opts.get()));
    listen(owner.get(), 3.0);
    let chunk = roots.root_nanbox(value_bytes(compressed));
    BOMB_CRC.with(|crc| *crc.borrow_mut() = flate2::Crc::new());
    BOMB_BOUNDS.with(|bounds| bounds.set((0, 0)));
    let baseline = rss();
    BOMB.with(|stats| stats.set((0, true, baseline)));
    unsafe { method(owner.get(), "end", &[chunk.get()]) };
    pump();
    assert_eq!(BOMB.with(std::cell::Cell::get).0, 1024);
    assert!(field(owner.get(), "bytesWritten") < compressed.len() as f64);
    assert!(field(owner.get(), "readableLength") <= 3072.0);
    assert!(native_bytes(owner.get()) < 100000);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        BOMB.with(std::cell::Cell::get).0,
        1024,
        "paused for fifty milliseconds"
    );
    // Collect while the unread compressed tail lives only in the stream.
    perry_runtime::gc::js_gc_collect();
    unsafe { method(owner.get(), "resume", &[]) };
    pump();
    let (count, valid, peak) = BOMB.with(std::cell::Cell::get);
    assert_eq!(count, 100_000_000);
    assert!(valid);
    assert_eq!(BOMB_CRC.with(|crc| crc.borrow().sum()), expected);
    let (readable_peak, native_peak) = BOMB_BOUNDS.with(std::cell::Cell::get);
    assert!(
        readable_peak <= 3072,
        "bounded readable queue: {readable_peak}"
    );
    assert!(
        native_peak < 100000,
        "bounded native workspace: {native_peak}"
    );
    // Cold runtime/collector pages are not a decompressor queue. Record the
    // clean-process RSS separately rather than an allocator-dependent bound;
    // the full-size TypeScript witness also measures a slow consumer's RSS.
    assert_eq!(native_bytes(owner.get()), 0);
    assert!(
        perry_runtime::gc::copying_minor_cycles() > copies_before,
        "copying minors actually ran"
    );
    eprintln!("Z2 bytes={count} baseline_rss={baseline} peak_rss={peak} rss_delta={} readable_peak={readable_peak} native_peak={native_peak}", peak.saturating_sub(baseline));
}
#[test]
fn brotli_destroy_inside_data_releases_before_gc_and_closes_once_later() {
    let _agent = super::OwnAgent::enter();
    clear();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    let owner = roots.root_nanbox(factory("BrotliCompress", opts.get()));
    listen(owner.get(), 2.0);
    let chunk = roots.root_nanbox(value_bytes(&vec![65; 1_000_000]));
    unsafe { method(owner.get(), "end", &[chunk.get()]) };
    pump();
    let (before, after) = RELEASE.with(|v| v.borrow().unwrap());
    assert!(before > 1000000);
    assert_eq!(after, 0);
    let events = EVENTS.with(|v| v.borrow().clone());
    assert_eq!(events.iter().filter(|e| *e == "data").count(), 1);
    assert_eq!(events.iter().filter(|e| *e == "close").count(), 1);
    assert_eq!(
        field(owner.get(), "_handle").to_bits(),
        JsValue::NULL.bits()
    );
}
#[test]
fn every_decoder_reports_corrupt_input_then_close() {
    let _agent = super::OwnAgent::enter();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    for name in [
        "Gunzip",
        "Inflate",
        "InflateRaw",
        "Unzip",
        "BrotliDecompress",
        "ZstdDecompress",
    ] {
        clear();
        let owner = roots.root_nanbox(factory(name, opts.get()));
        listen(owner.get(), 0.0);
        let chunk = roots.root_nanbox(value_bytes(&[255; 100]));
        unsafe { method(owner.get(), "end", &[chunk.get()]) };
        pump();
        assert_eq!(
            EVENTS.with(|v| v.borrow().clone()),
            ["error", "close"],
            "{name}"
        );
        assert_eq!(native_bytes(owner.get()), 0);
    }
}

#[test]
fn codec_sabotages_turn_their_runtime_witnesses_red() {
    if std::env::var("PERRY_TEST_ZLIB_SABOTAGE").is_ok() {
        return;
    }
    for (fault, witness) in [
        (
            "error_as_eof",
            "every_decoder_reports_corrupt_input_then_close",
        ),
        (
            "own_codec_methods",
            "eleven_codecs_are_deferred_runtime_transforms_and_release_on_completion",
        ),
        (
            "skip_registry_bootstrap",
            "a_fresh_factory_installs_its_inherited_methods_without_a_stream_import",
        ),
        (
            "release_in_finalizer_only",
            "fifty_thousand_churn_per_codec_releases_native_bytes_and_has_flat_rss",
        ),
        (
            "raw_one_shot_callback",
            "ten_thousand_one_shots_keep_captures_through_a_collection_before_run",
        ),
        (
            "unbounded_output",
            "real_gunzip_bomb_parks_on_pause_and_keeps_the_input_traced",
        ),
        (
            "release_in_finalizer_only",
            "brotli_destroy_inside_data_releases_before_gc_and_closes_once_later",
        ),
        (
            "keep_handle_field",
            "eleven_codecs_are_deferred_runtime_transforms_and_release_on_completion",
        ),
        (
            "corrupt_output",
            "eleven_codecs_are_deferred_runtime_transforms_and_release_on_completion",
        ),
    ] {
        let witness = format!("stream::runtime_tests::{witness}");
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &witness,
                "--nocapture",
                "--include-ignored",
                "--test-threads=1",
            ])
            .env("PERRY_TEST_ZLIB_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("running 1 test"),
            "missing witness {witness}"
        );
        assert!(!result.status.success(), "{fault} must turn {witness} red");
        eprintln!("zlib sabotage {fault}: RED");
    }
}

// The fixture's JS executes entirely in runtime helpers, whose stores emit
// barriers. Announce the same contract as a generated program so the witness
// exercises copying minors rather than the nonmoving no-codegen fallback.
struct CompiledBarriers;
impl CompiledBarriers {
    fn new() -> Self {
        perry_runtime::gc::js_gc_write_barriers_emitted(1);
        Self
    }
}
impl Drop for CompiledBarriers {
    fn drop(&mut self) {
        perry_runtime::gc::js_gc_write_barriers_emitted(0);
    }
}

fn rss() -> usize {
    std::fs::read_to_string("/proc/self/statm")
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<usize>()
        .unwrap()
        * 4096
}
#[test]
#[ignore = "full Z8: 50,000 completions per codec plus 50,000 immediate destroys"]
fn fifty_thousand_churn_per_codec_releases_native_bytes_and_has_flat_rss() {
    let _agent = super::OwnAgent::enter();
    clear();
    let _barriers = CompiledBarriers::new();
    let copies_before = perry_runtime::gc::copying_minor_cycles();
    driver::PAYLOAD_COUNTS.with(|n| n.set((0, 0)));
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    let input = vec![65; 1024];
    let gzip = crate::gzip_bytes(&input).unwrap();
    let deflate = crate::deflate_bytes(&input).unwrap();
    let raw = crate::deflate_raw_bytes_with(&input, Compression::default()).unwrap();
    let brotli = brotli_compress_bytes(&input);
    let zstd = zstd::stream::encode_all(input.as_slice(), 3).unwrap();
    let selected = std::env::var("PERRY_TEST_ZLIB_CHURN_CODEC").ok();
    let mut samples = Vec::new();
    for batch in 0..10 {
        for (name, input) in [
            ("Gzip", &input),
            ("Gunzip", &gzip),
            ("Deflate", &input),
            ("Inflate", &deflate),
            ("DeflateRaw", &input),
            ("InflateRaw", &raw),
            ("Unzip", &gzip),
            ("BrotliCompress", &input),
            ("BrotliDecompress", &brotli),
            ("ZstdCompress", &input),
            ("ZstdDecompress", &zstd),
        ] {
            if selected.as_ref().is_some_and(|filter| filter != name) {
                continue;
            }
            let started = std::time::Instant::now();
            for iteration in 0..5000 {
                let scope = TransientRootScope::enter();
                let owner = scope.root_nanbox(factory(name, opts.get()));
                assert_eq!(
                    field(owner.get(), "bytesWritten"),
                    0.0,
                    "constructor fields {name}, batch={batch} iteration={iteration}"
                );
                listen(owner.get(), 0.0);
                assert_eq!(
                    field(owner.get(), "bytesWritten"),
                    0.0,
                    "listener installation fields {name}, batch={batch} iteration={iteration}"
                );
                let chunk = scope.root_nanbox(value_bytes(input));
                unsafe {
                    method(owner.get(), "end", &[chunk.get()]);
                }
                pump();
                assert_eq!(native_bytes(owner.get()), 0, "completion released {name}");
                assert!(!OUTPUT.with(|v| v.borrow().is_empty()),
                    "consumed {name}, batch={batch} iteration={iteration} bytesWritten={} events={:?}",
                    field(owner.get(), "bytesWritten"), EVENTS.with(|v| v.borrow().clone()));
                OUTPUT.with(|v| v.borrow_mut().clear());
                EVENTS.with(|v| v.borrow_mut().clear());
            }
            perry_runtime::gc::js_gc_collect();
            eprintln!(
                "Z8 codec={name} batch={batch} elapsed={:?}",
                started.elapsed()
            );
        }
        for _ in 0..if selected
            .as_ref()
            .is_none_or(|filter| filter == "Gzip" || filter == "destroy")
        {
            5000
        } else {
            0
        } {
            let scope = TransientRootScope::enter();
            let owner = scope.root_nanbox(factory("Gzip", opts.get()));
            unsafe {
                method(owner.get(), "destroy", &[]);
            }
            assert_eq!(native_bytes(owner.get()), 0, "explicit destroy released");
            pump();
        }
        perry_runtime::gc::js_gc_collect();
        samples.push(rss());
        let (created, dropped) = driver::PAYLOAD_COUNTS.with(std::cell::Cell::get);
        assert_eq!(created, dropped, "all codec allocations released before GC");
        eprintln!(
            "Z8 batch={batch} created={created} dropped={dropped} rss={}",
            samples[batch]
        );
    }
    let warm = *samples[3..].iter().min().unwrap();
    let peak = *samples[3..].iter().max().unwrap();
    assert!(
        peak - warm < 4 << 20,
        "RSS plateau exceeded 4 MiB: {samples:?}"
    );
    assert!(
        perry_runtime::gc::copying_minor_cycles() > copies_before,
        "copying minors actually ran"
    );
    let expected = match selected.as_deref() {
        None => 600000,
        Some("Gzip") => 100000,
        _ => 50000,
    };
    assert_eq!(
        driver::PAYLOAD_COUNTS.with(std::cell::Cell::get),
        (expected, expected)
    );
}
