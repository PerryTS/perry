//! STREAM-PAYLOAD-DESIGN §6 witnesses for the runtime phase, driven by a
//! test-only `rot13` Transform family and a `count` Writable family, so every
//! contract runs without zlib. Each sabotage (`PERRY_TEST_STREAM_SABOTAGE`)
//! runs once in a child and must turn its witness red
//! (`every_stream_sabotage_makes_its_witness_red`).

use super::*;
use crate::native_payload::{self as np, NativePayloadFamily, PayloadPrototype, StreamPayload};
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

// ─── the rot13 family ──────────────────────────────────────────────────────

// Per test thread: a sibling test's streams never move these counts.
per_test_global! {
    pub(crate) static CREATED: AtomicUsize = AtomicUsize::new(0);
    pub(crate) static DROPPED: AtomicUsize = AtomicUsize::new(0);
    pub(crate) static RELEASED: AtomicUsize = AtomicUsize::new(0);
    pub(crate) static STEPS: AtomicUsize = AtomicUsize::new(0);
    pub(crate) static STEP_JOBS_SEEN: AtomicUsize = AtomicUsize::new(0);
    static Z4_DESTROYED_IN_DATA: AtomicUsize = AtomicUsize::new(0);
    static Z11_OVERRIDE_CALLS: AtomicUsize = AtomicUsize::new(0);
    static G2_TRANSFORM_CALLS: AtomicUsize = AtomicUsize::new(0);
}

/// The codec: rot13 over each input byte, `expand` copies of each output byte
/// (a decompression-bomb stand-in), at most `step_out` output bytes per step,
/// failing on `fail_byte`, with a `|end` trailer at `Final`.
pub(crate) struct Rot13 {
    scratch: Vec<u8>,
    step_out: usize,
    expand: usize,
    fail_byte: Option<u8>,
    final_done: bool,
    reserve: usize,
}

impl Drop for Rot13 {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

fn rot13_byte(b: u8) -> u8 {
    match b {
        b'a'..=b'z' => (b - b'a' + 13) % 26 + b'a',
        b'A'..=b'Z' => (b - b'A' + 13) % 26 + b'A',
        _ => b,
    }
}

pub(crate) fn rot13_bytes(input: &[u8], expand: usize) -> Vec<u8> {
    input
        .iter()
        .flat_map(|b| std::iter::repeat_n(rot13_byte(*b), expand))
        .collect()
}

unsafe extern "C" fn rot13_step(payload: *mut std::ffi::c_void, op: &StepIn, out: &mut StepOut) {
    STEPS.fetch_add(1, Ordering::SeqCst);
    let p = &mut *(payload as *mut Rot13);
    p.scratch.clear();
    if p.scratch.capacity() < p.reserve {
        p.scratch.reserve(p.reserve);
    }
    match op.op {
        StreamOp::WRITE => {
            let input: &[u8] = if op.input.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(op.input, op.len)
            };
            let take = (p.step_out / p.expand).max(1).min(input.len());
            if let Some(bad) = p.fail_byte {
                if input[..take].contains(&bad) {
                    out.status = StepStatus::ERROR;
                    out.code = 7;
                    out.external_bytes = p.scratch.capacity();
                    return;
                }
            }
            p.scratch.extend(rot13_bytes(&input[..take], p.expand));
            out.consumed = take;
            out.status = if take < input.len() {
                StepStatus::MORE
            } else {
                StepStatus::NEED_INPUT
            };
        }
        StreamOp::FLUSH => out.status = StepStatus::NEED_INPUT,
        _ => {
            if p.final_done {
                out.status = StepStatus::ENDED;
            } else {
                p.final_done = true;
                p.scratch.extend_from_slice(b"|end");
                out.status = StepStatus::MORE;
            }
        }
    }
    out.out = p.scratch.as_ptr();
    out.out_len = p.scratch.len();
    out.external_bytes = p.scratch.capacity();
}

unsafe extern "C" fn rot13_error(_owner: f64, code: u32) -> f64 {
    let message = format!("rot13: bad input byte (code {code})");
    let s = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_ROT13");
    let err = crate::error::js_error_new_with_message(s);
    crate::value::js_nanbox_pointer(err as i64)
}

unsafe extern "C" fn rot13_release(owner: f64) {
    if np::close_attached::<Rot13>(owner, &ROT13_FAMILY) {
        RELEASED.fetch_add(1, Ordering::SeqCst);
    }
}

pub(crate) static ROT13_HOOKS: StreamHooks = StreamHooks {
    kind: StreamKind::TRANSFORM,
    timing: StepTiming::DEFERRED,
    lazy: false,
    step: rot13_step,
    error: rot13_error,
    release: rot13_release,
};

impl StreamPayload for Rot13 {
    const HOOKS: &'static StreamHooks = &ROT13_HOOKS;
}

/// `#Rot13Base.prototype._transform(chunk, enc, cb)`: the same step, run on
/// the whole chunk with a JS callback (node's `ZlibBase.prototype._transform`
/// for `super._transform(...)`).
extern "C" fn rot13_proto_transform(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(this.as_f64());
    let cb = scope.root_nanbox_f64(cb);
    let mut bytes = Vec::new();
    append_chunk_bytes(chunk, &mut bytes, 0);
    let mut consumed = 0;
    loop {
        let Ok(payload) =
            (unsafe { np::payload_mut_attached::<Rot13>(stream.get_nanbox_f64(), &ROT13_FAMILY) })
        else {
            break;
        };
        let step_in = StepIn {
            op: StreamOp::WRITE,
            flush_kind: 0,
            input: unsafe { bytes.as_ptr().add(consumed) },
            len: bytes.len() - consumed,
        };
        let mut out = StepOut::empty();
        unsafe {
            rot13_step(
                payload as *mut Rot13 as *mut std::ffi::c_void,
                &step_in,
                &mut out,
            )
        };
        consumed += out.consumed;
        let produced = unsafe { std::slice::from_raw_parts(out.out, out.out_len) }.to_vec();
        if !produced.is_empty() {
            let buf = scope.root_nanbox_f64(buffer_value_from_bytes(&produced));
            let _ = push_chunk(stream.get_nanbox_f64(), buf.get_nanbox_f64());
        }
        if out.status != StepStatus::MORE {
            break;
        }
    }
    if is_callable_value(cb.get_nanbox_f64()) {
        call_listener_args(stream.get_nanbox_f64(), cb.get_nanbox_f64(), &[]);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn install_rot13(builder: &mut PayloadPrototype) {
    builder.inherit(super::proto_methods::stream_prototype_value("Transform"));
    builder.method(
        "_transform",
        crate::fn_info!(rot13_proto_transform, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    );
}

pub(crate) static ROT13_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: crate::native_class_ids::CRYPTO_CIPHERIV,
    links_owner: false,
    name: "Rot13",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: install_rot13,
};

pub(crate) struct Rot13Opts {
    pub step_out: usize,
    pub expand: usize,
    pub fail_byte: Option<u8>,
    pub reserve: usize,
    pub readable_hwm: Option<f64>,
    pub writable_hwm: Option<f64>,
    pub decode_strings: bool,
}

impl Default for Rot13Opts {
    fn default() -> Self {
        Self {
            step_out: 4096,
            expand: 1,
            fail_byte: None,
            reserve: 0,
            readable_hwm: None,
            writable_hwm: None,
            decode_strings: true,
        }
    }
}

fn options_object(o: &Rot13Opts) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_nanbox_f64(box_pointer(
        crate::object::js_object_alloc(0, 4) as *const u8
    ));
    if let Some(hwm) = o.readable_hwm {
        set_visible_own_value(
            obj.get_nanbox_f64(),
            hidden_key(b"readableHighWaterMark"),
            hwm,
        );
    }
    if let Some(hwm) = o.writable_hwm {
        set_visible_own_value(
            obj.get_nanbox_f64(),
            hidden_key(b"writableHighWaterMark"),
            hwm,
        );
    }
    if !o.decode_strings {
        set_visible_own_value(
            obj.get_nanbox_f64(),
            hidden_key(b"decodeStrings"),
            f64::from_bits(TAG_FALSE),
        );
    }
    obj.get_nanbox_f64()
}

/// `new Rot13(opts)`: allocate with the family prototype and the payload
/// (node's pre-fields: `_rotPre`), run Transform's constructor body, then the
/// post-fields (`_rotPost`), in node's constructor order.
pub(crate) fn new_rot13(o: Rot13Opts) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(options_object(&o));
    let payload = Rot13 {
        scratch: Vec::new(),
        step_out: o.step_out,
        expand: o.expand,
        fail_byte: o.fail_byte,
        final_done: false,
        reserve: o.reserve,
    };
    let stream = scope.root_nanbox_f64(np::alloc_stream(
        &ROT13_FAMILY,
        payload,
        o.reserve,
        &[(b"_rotPre", 1.0)],
    ));
    CREATED.fetch_add(1, Ordering::SeqCst);
    init_transform_in_place(stream.get_nanbox_f64(), opts.get_nanbox_f64());
    set_visible_own_value(stream.get_nanbox_f64(), hidden_key(b"_rotPost"), 2.0);
    stream.get_nanbox_f64()
}

/// Reset the family's prototype slot and counters (tests share a thread).
pub(crate) struct FamilyReset;
impl FamilyReset {
    pub(crate) fn new() -> Self {
        np::reset_payload_prototypes_for_tests();
        for c in [&CREATED, &DROPPED, &RELEASED, &STEPS, &STEP_JOBS_SEEN] {
            c.store(0, Ordering::SeqCst);
        }
        LOG.with(|l| l.borrow_mut().clear());
        DATA.with(|d| d.borrow_mut().clear());
        Self
    }
}
impl Drop for FamilyReset {
    fn drop(&mut self) {
        pump();
        np::reset_payload_prototypes_for_tests();
    }
}

// ─── driving helpers ───────────────────────────────────────────────────────

/// Run microtasks and immediates until both queues are idle.
pub(crate) fn pump() {
    for _ in 0..200_000 {
        let a = crate::promise::js_promise_run_microtasks();
        let b = crate::timer::js_event_loop_check_phase();
        if a == 0 && b == 0 {
            break;
        }
    }
}

/// One turn: microtasks, then one check phase, then microtasks.
fn turn() {
    crate::promise::js_promise_run_microtasks();
    crate::timer::js_event_loop_check_phase();
    crate::promise::js_promise_run_microtasks();
}

fn sv(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

/// An event or property name as compiled code passes one: an interned
/// literal (never a young string).
pub(crate) fn name(s: &str) -> f64 {
    let ptr = crate::string::intern_ascii_literal(s.as_bytes()) as *mut crate::StringHeader;
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn handle(stream: f64) -> i64 {
    raw_ptr_from_value(stream) as i64
}

fn bytes_of(value: f64) -> Vec<u8> {
    let mut out = Vec::new();
    append_chunk_bytes(value, &mut out, 0);
    out
}

fn closure0(info: *const crate::closure::JsFunctionInfo, captures: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let roots: Vec<_> = captures.iter().map(|c| scope.root_nanbox_f64(*c)).collect();
    let c = js_closure_alloc(info, roots.len() as u32);
    for (i, r) in roots.iter().enumerate() {
        js_closure_set_capture_f64(c, i as u32, r.get_nanbox_f64());
    }
    box_pointer(c as *const u8)
}

thread_local! {
    pub(crate) static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    pub(crate) static DATA: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static MAX_READABLE: RefCell<f64> = const { RefCell::new(0.0) };
    static PENDING_SINK_CBS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    static PRODUCER_BLOCKED: RefCell<bool> = const { RefCell::new(false) };
    static Z1_PAUSES: RefCell<usize> = const { RefCell::new(0) };
}

fn log(s: &str) {
    LOG.with(|l| l.borrow_mut().push(s.to_string()));
}

fn log_snapshot() -> Vec<String> {
    LOG.with(|l| l.borrow().clone())
}

extern "C" fn on_data(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    let bytes = bytes_of(chunk);
    DATA.with(|d| d.borrow_mut().extend_from_slice(&bytes));
    let _ = closure;
    log("data");
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn on_named(closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    let name = js_closure_get_capture_f64(closure, 0);
    let mut b = Vec::new();
    append_chunk_bytes(name, &mut b, 0);
    log(std::str::from_utf8(&b).unwrap());
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn on_error(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    let msg = bytes_of(unsafe { crate::object::js_object_get_property_key(err, name("message")) });
    log(&format!("error:{}", String::from_utf8_lossy(&msg)));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn on_write_cb(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    let jsv = JSValue::from_bits(err.to_bits());
    if jsv.is_null() || jsv.is_undefined() {
        log("cb");
    } else {
        log("cb:error");
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn listen(stream: f64, event: &str) {
    let l = closure0(crate::fn_info!(on_named, 0), &[name(event)]);
    js_node_stream_method_on(handle(stream), name(event), l);
}

fn listen_data(stream: f64) {
    let l = closure0(crate::fn_info!(on_data, 1), &[]);
    js_node_stream_method_on(handle(stream), name("data"), l);
}

fn listen_error(stream: f64) {
    let l = closure0(crate::fn_info!(on_error, 1), &[]);
    js_node_stream_method_on(handle(stream), name("error"), l);
}

fn write(stream: f64, chunk: f64, cb: Option<f64>) -> bool {
    js_node_stream_method_write(
        handle(stream),
        chunk,
        f64::from_bits(TAG_UNDEFINED),
        cb.unwrap_or(f64::from_bits(TAG_UNDEFINED)),
    )
    .to_bits()
        == TAG_TRUE
}

fn end(stream: f64) {
    js_node_stream_method_end3(
        handle(stream),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );
}

fn readable_length(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0)
}

fn input_bytes(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..n)
        .map(|_| {
            x = x
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            b'a' + ((x >> 33) % 26) as u8
        })
        .collect()
}

// ─── the slow sink of Z1/Z2 ────────────────────────────────────────────────

/// `_write(chunk, enc, cb)`: record the source's readable length, keep the
/// callback; the test completes one pending write per turn (a consumer that
/// takes its time per chunk).
extern "C" fn slow_sink_write(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let source = js_closure_get_capture_f64(closure, 0);
    let len = readable_length(source);
    MAX_READABLE.with(|m| {
        let mut m = m.borrow_mut();
        if len > *m {
            *m = len;
        }
    });
    DATA.with(|d| d.borrow_mut().extend_from_slice(&bytes_of(chunk)));
    PENDING_SINK_CBS.with(|p| p.borrow_mut().push(cb));
    f64::from_bits(TAG_UNDEFINED)
}

fn new_slow_sink(source: f64, hwm: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(source);
    let opts = scope.root_nanbox_f64(box_pointer(
        crate::object::js_object_alloc(0, 2) as *const u8
    ));
    let w = closure0(
        crate::fn_info!(slow_sink_write, 3; with_declared(3)),
        &[source.get_nanbox_f64()],
    );
    set_visible_own_value(opts.get_nanbox_f64(), hidden_key(b"write"), w);
    set_visible_own_value(opts.get_nanbox_f64(), hidden_key(b"highWaterMark"), hwm);
    js_node_stream_writable_new(opts.get_nanbox_f64())
}

/// Complete one pending sink write (its callback runs asynchronously, as a
/// consumer that finished later).
fn complete_one_sink_write() -> bool {
    let cb = PENDING_SINK_CBS.with(|p| {
        let mut p = p.borrow_mut();
        (!p.is_empty()).then(|| p.remove(0))
    });
    let Some(cb) = cb else {
        return false;
    };
    unsafe {
        let _ = crate::closure::js_native_call_value(
            cb,
            crate::closure::plain_call_receiver(),
            [f64::from_bits(TAG_NULL)].as_ptr(),
            1,
        );
    }
    true
}

/// The largest readable length of the codec stream seen so far (sampled
/// after every producer write, every turn and every sink completion).
fn sample_readable(stream: f64) {
    if readable_is_paused(stream) {
        Z1_PAUSES.with(|p| *p.borrow_mut() += 1);
    }
    let len = readable_length(stream);
    MAX_READABLE.with(|m| {
        let mut m = m.borrow_mut();
        if len > *m {
            *m = len;
        }
    });
}

extern "C" fn on_drain_unblock(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    PRODUCER_BLOCKED.with(|b| *b.borrow_mut() = false);
    f64::from_bits(TAG_UNDEFINED)
}

/// Drive `input` through `rot13.pipe(sink)` with a producer that honours
/// `write() === false` and a sink that completes one write per turn. Returns
/// the output and the largest readable length of rot13 seen at a sink write.
fn drive_backpressure(o: Rot13Opts, input: &[u8], chunk: usize, sink_hwm: f64) -> (Vec<u8>, f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    MAX_READABLE.with(|m| *m.borrow_mut() = 0.0);
    PENDING_SINK_CBS.with(|p| p.borrow_mut().clear());
    PRODUCER_BLOCKED.with(|b| *b.borrow_mut() = false);
    DATA.with(|d| d.borrow_mut().clear());
    let rot = scope.root_nanbox_f64(new_rot13(o));
    let sink = scope.root_nanbox_f64(new_slow_sink(rot.get_nanbox_f64(), sink_hwm));
    let unblock = closure0(crate::fn_info!(on_drain_unblock, 0), &[]);
    js_node_stream_method_on(handle(rot.get_nanbox_f64()), name("drain"), unblock);
    js_node_stream_method_pipe(
        handle(rot.get_nanbox_f64()),
        sink.get_nanbox_f64(),
        f64::from_bits(TAG_UNDEFINED),
    );
    let mut offset = 0;
    let mut ended = false;
    for iteration in 0..2_000_000u64 {
        if !PRODUCER_BLOCKED.with(|b| *b.borrow()) && offset < input.len() {
            let next = (offset + chunk).min(input.len());
            let buf = buffer_value_from_bytes(&input[offset..next]);
            offset = next;
            if !write(rot.get_nanbox_f64(), buf, None) {
                PRODUCER_BLOCKED.with(|b| *b.borrow_mut() = true);
            }
        } else if offset >= input.len() && !ended {
            end(rot.get_nanbox_f64());
            ended = true;
        }
        sample_readable(rot.get_nanbox_f64());
        turn();
        sample_readable(rot.get_nanbox_f64());
        // The consumer is four times slower than the producer.
        let progressed = if iteration % 4 == 0 || offset >= input.len() {
            complete_one_sink_write()
        } else {
            true
        };
        turn();
        sample_readable(rot.get_nanbox_f64());
        if ended
            && !progressed
            && crate::timer::js_immediate_has_pending() == 0
            && PENDING_SINK_CBS.with(|p| p.borrow().is_empty())
        {
            break;
        }
    }
    pump();
    (
        DATA.with(|d| d.borrow().clone()),
        MAX_READABLE.with(|m| *m.borrow()),
    )
}

// ─── Z1: backpressure parks the codec ──────────────────────────────────────

#[test]
fn z1_pipe_backpressure_parks_the_codec_within_hwm_plus_one_step() {
    let _reset = FamilyReset::new();
    let input = input_bytes(256 * 1024, 1);
    let hwm = 4096.0;
    let step_out = 1024;
    let (output, max_readable) = drive_backpressure(
        Rot13Opts {
            step_out,
            readable_hwm: Some(hwm),
            writable_hwm: Some(4096.0),
            ..Default::default()
        },
        &input,
        1024,
        2048.0,
    );
    let mut expected = rot13_bytes(&input, 1);
    expected.extend_from_slice(b"|end");
    assert_eq!(output.len(), expected.len(), "every byte arrives");
    assert!(output == expected, "bytes equal rot13(input)");
    eprintln!(
        "z1: max readableLength = {max_readable} (hwm {hwm}, step {step_out}), pauses = {}",
        Z1_PAUSES.with(|p| *p.borrow())
    );
    assert!(
        max_readable <= hwm + step_out as f64,
        "the readable side stays within hwm + one step: {max_readable}"
    );
}

// ─── Z2: an expanding codec parks mid-input ────────────────────────────────

#[test]
fn z2_expanding_input_parks_before_its_input_is_exhausted() {
    let _reset = FamilyReset::new();
    // 1 KiB in, 1 MiB out: one write expands 1024x.
    let input = input_bytes(1024, 2);
    let hwm = 16384.0;
    let step_out = 4096;
    let (output, max_readable) = drive_backpressure(
        Rot13Opts {
            step_out,
            expand: 1024,
            readable_hwm: Some(hwm),
            ..Default::default()
        },
        &input,
        1024,
        16384.0,
    );
    let mut expected = rot13_bytes(&input, 1024);
    expected.extend_from_slice(b"|end");
    assert!(
        output == expected,
        "output correct ({} bytes)",
        output.len()
    );
    eprintln!("z2: max readableLength = {max_readable} (hwm {hwm})");
    assert!(
        max_readable <= hwm + step_out as f64,
        "a 1024x expansion stays bounded by hwm + one step: {max_readable}"
    );
}

// ─── Z3: four ways in, one trace out ───────────────────────────────────────

extern "C" fn source_read_noop(
    _c: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _n: f64,
) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn new_source() -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(box_pointer(
        crate::object::js_object_alloc(0, 1) as *const u8
    ));
    let r = closure0(crate::fn_info!(source_read_noop, 1; with_declared(1)), &[]);
    set_visible_own_value(opts.get_nanbox_f64(), hidden_key(b"read"), r);
    js_node_stream_readable_new(opts.get_nanbox_f64())
}

fn trace_way(way: u8, chunks: &[Vec<u8>]) -> (Vec<u8>, Vec<String>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    LOG.with(|l| l.borrow_mut().clear());
    DATA.with(|d| d.borrow_mut().clear());
    let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts {
        step_out: 64,
        ..Default::default()
    }));
    listen_data(rot.get_nanbox_f64());
    for ev in ["finish", "end", "close"] {
        listen(rot.get_nanbox_f64(), ev);
    }
    match way {
        // write()/end()
        0 => {
            for c in chunks {
                write(rot.get_nanbox_f64(), buffer_value_from_bytes(c), None);
            }
            end(rot.get_nanbox_f64());
        }
        // source.pipe(rot)
        1 => {
            let src = scope.root_nanbox_f64(new_source());
            js_node_stream_method_pipe(
                handle(src.get_nanbox_f64()),
                rot.get_nanbox_f64(),
                f64::from_bits(TAG_UNDEFINED),
            );
            for c in chunks {
                js_node_stream_method_push(
                    handle(src.get_nanbox_f64()),
                    buffer_value_from_bytes(c),
                );
            }
            js_node_stream_method_push(handle(src.get_nanbox_f64()), f64::from_bits(TAG_NULL));
        }
        // pipeline(source, rot)
        _ => {
            let src = scope.root_nanbox_f64(new_source());
            let mut args = crate::array::js_array_alloc(2);
            args = crate::array::js_array_push_f64(args, src.get_nanbox_f64());
            args = crate::array::js_array_push_f64(args, rot.get_nanbox_f64());
            let done = closure0(crate::fn_info!(on_named, 0), &[name("pipeline-done")]);
            args = crate::array::js_array_push_f64(args, done);
            let args = scope.root_raw_mut_ptr(args);
            js_node_stream_pipeline(args.get_raw_mut_ptr::<crate::array::ArrayHeader>());
            for c in chunks {
                js_node_stream_method_push(
                    handle(src.get_nanbox_f64()),
                    buffer_value_from_bytes(c),
                );
            }
            js_node_stream_method_push(handle(src.get_nanbox_f64()), f64::from_bits(TAG_NULL));
        }
    }
    pump();
    let mut events: Vec<String> = log_snapshot()
        .into_iter()
        .filter(|e| e != "pipeline-done")
        .collect();
    // Collapse the data run: the trace is the order of the terminal events
    // relative to the data.
    events.dedup();
    (DATA.with(|d| d.borrow().clone()), events)
}

#[test]
fn z3_write_pipe_and_pipeline_give_identical_bytes_and_event_order() {
    let _reset = FamilyReset::new();
    let input = input_bytes(10_000, 3);
    let chunks: Vec<Vec<u8>> = input.chunks(700).map(|c| c.to_vec()).collect();
    let mut expected = rot13_bytes(&input, 1);
    expected.extend_from_slice(b"|end");
    let mut traces = Vec::new();
    for way in 0..3u8 {
        let (bytes, events) = trace_way(way, &chunks);
        assert!(
            bytes == expected,
            "way {way}: bytes equal rot13(input) ({} vs {})",
            bytes.len(),
            expected.len()
        );
        traces.push(events);
    }
    eprintln!("z3 traces: {traces:?}");
    assert_eq!(traces[0], traces[1], "write/end and pipe agree");
    assert_eq!(traces[0], traces[2], "write/end and pipeline agree");
    assert_eq!(
        traces[0].last().map(String::as_str),
        Some("close"),
        "close is last"
    );
    let finish = traces[0].iter().position(|e| e == "finish").unwrap();
    let end = traces[0].iter().position(|e| e == "end").unwrap();
    assert!(
        end < finish,
        "node's Transform order: push(null) queues end before finish, then close"
    );
}

// ─── Z4: destroy inside `data` releases now, closes once, later ────────────

extern "C" fn destroy_on_first_data(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _chunk: f64,
) -> f64 {
    log("data");
    if Z4_DESTROYED_IN_DATA.fetch_add(1, Ordering::SeqCst) == 0 {
        let stream = js_closure_get_capture_f64(closure, 0);
        js_node_stream_method_destroy(handle(stream), f64::from_bits(TAG_UNDEFINED));
        // Synchronously after destroy(): the codec is released, before any
        // collection, and no event has been delivered yet.
        let cell = crate::native_payload::stream_hooks_of(stream).map(|(_, cell)| cell);
        let bytes = cell
            .map(|c| unsafe { (*c).external_bytes })
            .unwrap_or(u64::MAX);
        let closes = log_snapshot().iter().filter(|e| *e == "close").count();
        log(&format!(
            "after-destroy bytes={bytes} released={} closes={closes}",
            RELEASED.load(Ordering::SeqCst)
        ));
        // A `close` listener added after destroy() fires too (node 26.5.1).
        listen(stream, "close");
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn z4_destroy_in_data_releases_before_gc_and_closes_once_asynchronously() {
    let _reset = FamilyReset::new();
    Z4_DESTROYED_IN_DATA.store(0, Ordering::SeqCst);
    let scope = crate::gc::RuntimeHandleScope::new();
    let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts {
        step_out: 1024,
        reserve: 4 << 20,
        ..Default::default()
    }));
    let l = closure0(
        crate::fn_info!(destroy_on_first_data, 1),
        &[rot.get_nanbox_f64()],
    );
    js_node_stream_method_on(handle(rot.get_nanbox_f64()), name("data"), l);
    listen(rot.get_nanbox_f64(), "close");
    write(
        rot.get_nanbox_f64(),
        buffer_value_from_bytes(&input_bytes(64 * 1024, 4)),
        None,
    );
    pump();
    let events = log_snapshot();
    eprintln!("z4: {events:?}");
    assert_eq!(
        events,
        [
            "data",
            "after-destroy bytes=0 released=1 closes=0",
            "close",
            "close"
        ],
        "released synchronously; no data after destroy; one async close seen \
         by the early and the late listener"
    );
    assert_eq!(
        DROPPED.load(Ordering::SeqCst),
        1,
        "the codec dropped at destroy"
    );
}

// ─── Z5: a codec error destroys with node's error; the write cb sees it ───

#[test]
fn z5_codec_error_emits_error_then_close_and_fails_the_write() {
    let _reset = FamilyReset::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts {
        fail_byte: Some(b'#'),
        ..Default::default()
    }));
    listen_error(rot.get_nanbox_f64());
    listen(rot.get_nanbox_f64(), "close");
    listen(rot.get_nanbox_f64(), "end");
    let cb = closure0(crate::fn_info!(on_write_cb, 1), &[]);
    write(rot.get_nanbox_f64(), sv("abc#def"), Some(cb));
    pump();
    let events = log_snapshot();
    eprintln!("z5: {events:?}");
    assert_eq!(
        events,
        vec![
            "cb:error".to_string(),
            "error:rot13: bad input byte (code 7)".to_string(),
            "close".to_string()
        ]
    );
}

// ─── Z7: shape — no own methods, state non-enumerable, node's key order ────

fn own_keys(value: f64) -> Vec<String> {
    let arr = crate::object::js_object_keys(
        raw_ptr_from_value(value) as *const crate::object::ObjectHeader
    );
    let n = crate::array::js_array_length(arr);
    (0..n)
        .map(|i| {
            String::from_utf8_lossy(&bytes_of(crate::array::js_array_get_f64(arr, i))).into_owned()
        })
        .collect()
}

#[test]
fn z7_payload_stream_owns_only_state_and_its_fields_in_constructor_order() {
    let _reset = FamilyReset::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let rot = scope.root_nanbox_f64(new_rot13(Rot13Opts::default()));
    assert_eq!(
        own_keys(rot.get_nanbox_f64()),
        [
            "_rotPre",
            "_readableState",
            "_writableState",
            "allowHalfOpen",
            "_rotPost"
        ],
        "node's own keys, in constructor order; no method, no runtime state"
    );
    let proto = crate::object::js_object_get_prototype_of(rot.get_nanbox_f64());
    let names = crate::object::js_object_get_own_property_names(proto);
    let arr = raw_ptr_from_value(names) as *const crate::array::ArrayHeader;
    let names: Vec<String> = (0..crate::array::js_array_length(arr))
        .map(|i| {
            String::from_utf8_lossy(&bytes_of(crate::array::js_array_get_f64(arr, i))).into_owned()
        })
        .collect();
    assert_eq!(names, ["_transform", "constructor"]);
    // The methods are inherited (Transform -> Duplex -> Readable).
    for m in ["write", "end", "pipe", "on", "push"] {
        let v = unsafe { crate::object::js_object_get_property_key(rot.get_nanbox_f64(), name(m)) };
        assert!(is_callable_value(v), "{m} is inherited");
    }
    assert!(is_classic_stream_instance_of(
        rot.get_nanbox_f64(),
        "Transform"
    ));
    assert!(is_classic_stream_instance_of(
        rot.get_nanbox_f64(),
        "Writable"
    ));
    // A plain JS Transform has the same own-key surface.
    let t = scope.root_nanbox_f64(js_node_stream_transform_new(f64::from_bits(TAG_UNDEFINED)));
    assert_eq!(
        own_keys(t.get_nanbox_f64()),
        ["_readableState", "_writableState", "allowHalfOpen"]
    );
}

// ─── Z11: a JS `_transform` override wins, and `super._transform` runs the step

extern "C" fn subclass_transform_calls_super(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    chunk: f64,
    enc: f64,
    cb: f64,
) -> f64 {
    Z11_OVERRIDE_CALLS.fetch_add(1, Ordering::SeqCst);
    // `super._transform(chunk, enc, cb)`: the family prototype's builtin.
    rot13_proto_transform(std::ptr::null(), this, chunk, enc, cb)
}

#[test]
fn z11_subclass_transform_override_wins_and_super_runs_the_codec() {
    let _reset = FamilyReset::new();
    Z11_OVERRIDE_CALLS.store(0, Ordering::SeqCst);
    let scope = crate::gc::RuntimeHandleScope::new();
    // `class Sub extends Rot13 { _transform(c, e, cb) { super._transform(c, e, cb) } }`
    let family_proto = scope.root_nanbox_f64(np::prototype_value(&ROT13_FAMILY));
    let sub_proto = scope.root_nanbox_f64(crate::object::js_object_create(
        family_proto.get_nanbox_f64(),
    ));
    let over = closure0(
        crate::fn_info!(subclass_transform_calls_super, 3; with_declared(3)),
        &[],
    );
    set_visible_own_value(sub_proto.get_nanbox_f64(), hidden_key(b"_transform"), over);
    let obj = scope.root_nanbox_f64(crate::object::js_object_create(sub_proto.get_nanbox_f64()));
    assert!(np::attach_stream_to_object(
        obj.get_nanbox_f64(),
        &ROT13_FAMILY,
        Rot13 {
            scratch: Vec::new(),
            step_out: 16,
            expand: 1,
            fail_byte: None,
            final_done: false,
            reserve: 0,
        },
        0
    ));
    init_transform_in_place(obj.get_nanbox_f64(), f64::from_bits(TAG_UNDEFINED));
    listen_data(obj.get_nanbox_f64());
    write(obj.get_nanbox_f64(), sv("hello world, stream"), None);
    write(obj.get_nanbox_f64(), sv("second chunk"), None);
    pump();
    assert_eq!(
        Z11_OVERRIDE_CALLS.load(Ordering::SeqCst),
        2,
        "the override ran per write"
    );
    let out = DATA.with(|d| d.borrow().clone());
    assert_eq!(out, rot13_bytes(b"hello world, streamsecond chunk", 1));
}

// ─── G2 on a plain JS Transform: serialized writes, kCallback ──────────────

extern "C" fn sync_identity_transform(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    G2_TRANSFORM_CALLS.fetch_add(1, Ordering::SeqCst);
    log("xf");
    unsafe {
        let _ = crate::closure::js_native_call_value(
            cb,
            crate::closure::plain_call_receiver(),
            [f64::from_bits(TAG_NULL), chunk].as_ptr(),
            2,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn g2_js_transform_holds_its_callback_while_the_readable_side_is_full() {
    let _reset = FamilyReset::new();
    G2_TRANSFORM_CALLS.store(0, Ordering::SeqCst);
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(box_pointer(
        crate::object::js_object_alloc(0, 3) as *const u8
    ));
    let xf = closure0(
        crate::fn_info!(sync_identity_transform, 3; with_declared(3)),
        &[],
    );
    set_visible_own_value(opts.get_nanbox_f64(), hidden_key(b"transform"), xf);
    set_visible_own_value(
        opts.get_nanbox_f64(),
        hidden_key(b"readableHighWaterMark"),
        2.0,
    );
    set_visible_own_value(
        opts.get_nanbox_f64(),
        hidden_key(b"writableHighWaterMark"),
        2.0,
    );
    let t = scope.root_nanbox_f64(js_node_stream_transform_new(opts.get_nanbox_f64()));
    let mut rets = Vec::new();
    for x in ["1", "2", "3", "4"] {
        let cb = closure0(crate::fn_info!(on_write_cb, 1), &[]);
        rets.push(write(t.get_nanbox_f64(), sv(x), Some(cb)));
    }
    // node: xf 1, xf 2 ran; 3 and 4 wait behind the held callback of 2.
    assert_eq!(rets, [true, true, false, false]);
    assert_eq!(G2_TRANSFORM_CALLS.load(Ordering::SeqCst), 2);
    pump();
    assert_eq!(
        log_snapshot(),
        ["xf", "xf", "cb"],
        "only the first write completed"
    );
    assert_eq!(readable_length(t.get_nanbox_f64()), 2.0);
    assert_eq!(writable_length(t.get_nanbox_f64()), 3.0);
    // read() drains below the mark: the held callback runs and write 3
    // starts (node's exact interleaving is diffed by the gap test).
    let _ = js_node_stream_method_read(handle(t.get_nanbox_f64()), f64::from_bits(TAG_UNDEFINED));
    pump();
    assert!(
        G2_TRANSFORM_CALLS.load(Ordering::SeqCst) >= 3,
        "write 3 started"
    );
    assert_eq!(
        log_snapshot().iter().filter(|e| *e == "cb").count(),
        2,
        "the held completion of write 2 ran"
    );
}

// ─── the sabotage children ─────────────────────────────────────────────────

#[test]
fn every_stream_sabotage_makes_its_witness_red() {
    if std::env::var("PERRY_TEST_STREAM_SABOTAGE").is_ok() {
        return;
    }
    let mut table = Vec::new();
    for (fault, witness) in [
        ("never_park", "node_stream::native_hooks::tests::z1_pipe_backpressure_parks_the_codec_within_hwm_plus_one_step"),
        ("exhaust_before_park", "node_stream::native_hooks::tests::z2_expanding_input_parks_before_its_input_is_exhausted"),
        ("pipe_bypass_writing", "node_stream::native_hooks::tests::z3_write_pipe_and_pipeline_give_identical_bytes_and_event_order"),
        ("release_in_finalizer_only", "node_stream::native_hooks::tests::z4_destroy_in_data_releases_before_gc_and_closes_once_asynchronously"),
        ("close_sync", "node_stream::native_hooks::tests::z4_destroy_in_data_releases_before_gc_and_closes_once_asynchronously"),
        ("error_as_end", "node_stream::native_hooks::tests::z5_codec_error_emits_error_then_close_and_fails_the_write"),
        ("own_methods", "node_stream::native_hooks::tests::z7_payload_stream_owns_only_state_and_its_fields_in_constructor_order"),
        ("enumerable_state", "node_stream::native_hooks::tests::z7_payload_stream_owns_only_state_and_its_fields_in_constructor_order"),
        ("hooks_first", "node_stream::native_hooks::tests::z11_subclass_transform_override_wins_and_super_runs_the_codec"),
        ("skip_release_autodestroy", "gc::tests::native_payload_streams::z8_churn_releases_every_codec_at_completion_and_drops_every_payload"),
        ("keep_step_closure", "gc::tests::native_payload_streams::z8_churn_releases_every_codec_at_completion_and_drops_every_payload"),
        ("hold_slice_across_push", "gc::tests::native_payload_streams::z9_moving_gc_inside_data_keeps_the_output_correct"),
        ("drain_after_teardown", "gc::tests::native_payload_streams::z10_worker_exit_with_a_step_queued_runs_no_step"),
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", witness, "--nocapture", "--test-threads=1"])
            .env("PERRY_TEST_STREAM_SABOTAGE", fault)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "{witness} must exist"
        );
        let red = !output.status.success();
        eprintln!("stream sabotage {fault} -> {witness}: {}", if red { "RED" } else { "green" });
        table.push((fault, red));
    }
    let green: Vec<_> = table
        .iter()
        .filter(|(_, red)| !red)
        .map(|(f, _)| *f)
        .collect();
    assert!(
        green.is_empty(),
        "sabotages that left their witness green: {green:?}"
    );
}

// ─── test seams for the gc witnesses ──────────────────────────────────────
