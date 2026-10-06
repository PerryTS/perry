//! Codec witnesses through the real runtime Transform, rather than a mock runner.
use super::*;
use std::cell::RefCell;

thread_local! {
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static RELEASE: RefCell<Option<(usize, usize)>> = const { RefCell::new(None) };
}
extern "C" {
    fn js_nm_install_zlib();
}
fn clear() {
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
    OUTPUT.with(|v| v.borrow_mut().extend(bytes));
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
fn options() -> f64 {
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    np::own(opts.get(), "chunkSize", 1024.0);
    np::own(opts.get(), "readableHighWaterMark", 2048.0);
    opts.get()
}
#[test]
fn eleven_codecs_are_deferred_runtime_transforms_and_release_on_completion() {
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
    clear();
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(options());
    let input = vec![65; 16_000_000];
    let compressed = crate::gzip_bytes(&input).unwrap();
    let owner = roots.root_nanbox(factory("Gunzip", opts.get()));
    listen(owner.get(), 1.0);
    let chunk = roots.root_nanbox(value_bytes(&compressed));
    unsafe { method(owner.get(), "end", &[chunk.get()]) };
    pump();
    assert!(OUTPUT.with(|v| v.borrow().len()) <= 1024);
    assert!(field(owner.get(), "bytesWritten") < compressed.len() as f64);
    assert!(field(owner.get(), "readableLength") <= 3072.0);
    assert!(native_bytes(owner.get()) < 100000);
    unsafe { method(owner.get(), "destroy", &[]) };
    assert_eq!(native_bytes(owner.get()), 0);
    pump();
}
#[test]
fn brotli_destroy_inside_data_releases_before_gc_and_closes_once_later() {
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
