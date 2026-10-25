//! `node:zlib` option-object helpers shared with the `perry-ext-zlib` codec
//! crate.
//!
//! The one-shot codecs (`gzipSync`/`deflateSync`/…) live in `perry-ext-zlib`
//! (a `#[no_mangle]` C-ABI crate, see `well_known_bindings.toml`). That crate
//! has no access to Perry's by-name object reader or the RangeError-throwing
//! machinery, so it calls back into this helper to resolve + validate the
//! `level` option (#2935). Keeping validation here means an invalid `level`
//! throws a Node-compatible `RangeError [ERR_OUT_OF_RANGE]` via the normal
//! `js_throw` path rather than silently clamping inside the ext crate.

/// Resolve a `node:zlib` `{ level }` option to a `flate2` compression level
/// (`0..=9`), validating against Node's `-1..=9` accepted range.
///
/// `opts` is the raw NaN-boxed options value passed to a one-shot codec. When
/// it is not an object, or carries no (or `undefined`) `level`, the zlib
/// default level (`6`) is returned. Node's `Z_DEFAULT_COMPRESSION` (`-1`) maps
/// to the same default. A `level` outside `-1..=9` throws
/// `RangeError [ERR_OUT_OF_RANGE]` before any compression runs.
#[no_mangle]
pub extern "C" fn js_zlib_resolve_level(opts: f64) -> i32 {
    const DEFAULT_LEVEL: i32 = 6;

    let jv = crate::value::JSValue::from_bits(opts.to_bits());
    if !jv.is_pointer() {
        return DEFAULT_LEVEL;
    }
    let ptr = jv.as_pointer::<crate::object::ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return DEFAULT_LEVEL;
    }

    let key = crate::string::js_string_from_bytes(b"level".as_ptr(), 5);
    let level_value = crate::object::js_object_get_field_by_name_f64(ptr, key);
    let lv = crate::value::JSValue::from_bits(level_value.to_bits());
    if lv.is_undefined() || lv.is_null() {
        return DEFAULT_LEVEL;
    }

    let level = if lv.is_int32() {
        lv.as_int32()
    } else if lv.is_number() {
        f64::from_bits(level_value.to_bits()) as i32
    } else {
        // Non-numeric `level` — fall back to the default rather than throwing
        // a type error (the parity surface here is numeric out-of-range).
        return DEFAULT_LEVEL;
    };

    if !(-1..=9).contains(&level) {
        let message = format!(
            "The value of \"options.level\" is out of range. It must be >= -1 and <= 9. Received {level}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }

    if level < 0 {
        DEFAULT_LEVEL
    } else {
        level
    }
}

/// Validate the `level`/`strategy` arguments to a zlib stream's
/// `.params(level, strategy, cb)` (#3285) and return the clamped flate2
/// compression level (`0..=9`).
///
/// Both args arrive NaN-boxed exactly as passed from JS. Mirroring Node:
/// a non-numeric `level` or `strategy` throws `TypeError [ERR_INVALID_ARG_TYPE]`;
/// a `level` outside `-1..=9` or a `strategy` outside `0..=4` throws
/// `RangeError [ERR_OUT_OF_RANGE]`. The level argument is validated first.
/// `Z_DEFAULT_COMPRESSION` (`-1`) maps to the zlib default level (`6`). The
/// `strategy` value is validated but not otherwise applied (flate2 exposes no
/// strategy knob); validation parity is the observable behavior.
///
/// The ext-zlib crate can't reach Perry's number/string typing or the
/// `js_throw` machinery, so it calls back here just like `js_zlib_resolve_level`.
#[no_mangle]
pub extern "C" fn js_zlib_validate_params(level: f64, strategy: f64) -> i32 {
    const DEFAULT_LEVEL: i32 = 6;

    fn as_number(v: f64, arg: &str) -> f64 {
        let jv = crate::value::JSValue::from_bits(v.to_bits());
        if jv.is_int32() {
            jv.as_int32() as f64
        } else if jv.is_number() {
            f64::from_bits(v.to_bits())
        } else {
            let received = crate::fs::validate::describe_received(v);
            let message =
                format!("The \"{arg}\" argument must be of type number. Received {received}");
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        }
    }

    let level_num = as_number(level, "level");
    let strategy_num = as_number(strategy, "strategy");

    let level_i = level_num as i32;
    if !(-1..=9).contains(&level_i) {
        let message = format!(
            "The value of \"level\" is out of range. It must be >= -1 and <= 9. Received {level_i}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }

    let strategy_i = strategy_num as i32;
    if !(0..=4).contains(&strategy_i) {
        let message = format!(
            "The value of \"strategy\" is out of range. It must be >= 0 and <= 4. Received {strategy_i}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }

    if level_i < 0 {
        DEFAULT_LEVEL
    } else {
        level_i
    }
}

/// Read an options-object field by name as a raw NaN-boxed `f64`. Returns the
/// `undefined` sentinel when `ptr` carries no such property.
fn read_option_field(ptr: *const crate::object::ObjectHeader, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ptr = scope.root_raw_const_ptr(ptr);
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        name.as_ptr(),
        name.len() as u32,
    ));
    key.with_const_ptr(|key| {
        ptr.with_const_ptr(|ptr| crate::object::js_object_get_field_by_name_f64(ptr, key))
    })
}

/// Decode an options field to its numeric `f64`, validating Node's type
/// contract: an absent/`undefined`/`null` value is skipped (`None`), a numeric
/// value is returned, and anything else throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with Node's `The "<name>" property must be of type number. Received …` shape.
fn option_number(value: f64, name: &str) -> Option<f64> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return None;
    }
    if jv.is_int32() {
        return Some(jv.as_int32() as f64);
    }
    if jv.is_number() {
        return Some(f64::from_bits(value.to_bits()));
    }
    let received = crate::fs::validate::describe_received(value);
    let message = format!("The \"{name}\" property must be of type number. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Validate a `>= min && <= max` zlib option (`level`/`memLevel`/`strategy`/
/// `windowBits`/`flush`). Mirrors Node's `checkRangesOrGetDefault`: a present
/// numeric value outside `[min, max]` throws `RangeError [ERR_OUT_OF_RANGE]`.
fn validate_option_range(
    ptr: *const crate::object::ObjectHeader,
    name: &[u8],
    display: &str,
    min: i64,
    max: i64,
    zero_is_default: bool,
) {
    let Some(n) = option_number(read_option_field(ptr, name), display) else {
        return;
    };
    if zero_is_default && n == 0.0 {
        return;
    }
    if n < min as f64 || n > max as f64 {
        let received = crate::fs::validate::format_received_number(n);
        let message = format!(
            "The value of \"{display}\" is out of range. It must be >= {min} and <= {max}. Received {received}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
}

/// Resolve `chunkSize` consistently for option validation and stream allocation.
/// Undefined and NaN use Node's default; all other values must be finite
/// numbers >= Z_MIN_CHUNK (64). Fractional sizes remain accepted.
fn validate_option_chunk_size(value: f64) -> f64 {
    if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        return 16384.0;
    }
    let n = crate::validators::validate_number(value, "options.chunkSize", None, None);
    if n.is_nan() {
        return 16384.0;
    }
    if !n.is_finite() {
        let received = crate::fs::validate::format_received_number(n);
        let message = format!(
            "The value of \"options.chunkSize\" is out of range. It must be a finite number. Received {received}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    if n < 64.0 {
        let received = crate::fs::validate::format_received_number(n);
        let message = format!(
            "The value of \"options.chunkSize\" is out of range. It must be >= 64. Received {received}"
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    n
}

/// Validate a `node:zlib` options object the way Node's `Zlib`/`ZlibBase`
/// constructors do, throwing the spec-mandated `TypeError`/`RangeError` before
/// any compression runs (#3662). Shared by the one-shot sync codecs and the
/// `createGzip`/`createDeflate`/… stream factories, and by both the in-tree
/// (`perry-stdlib`) and out-of-tree (`perry-ext-zlib`) codec crates.
///
/// `min_window_bits` is the lower `windowBits` bound, which differs by codec:
/// gzip compression requires `>= 9`, every other codec accepts `>= 8`.
/// Policy zero additionally permits exactly zero for Inflate/Gunzip/Unzip;
/// their nonzero lower bound remains 8.
///
/// Preserve the existing ranged-option order (`windowBits`, `level`,
/// `memLevel`, `strategy`, `chunkSize`, `flush`) before validating `dictionary`.
/// A present `dictionary` must have a public Buffer/TypedArray/DataView or
/// ArrayBuffer brand. Valid dictionaries still follow the deferred-application
/// warning path; validation does not apply a preset dictionary to the codec.
#[no_mangle]
pub extern "C" fn js_zlib_validate_options(opts: f64, min_window_bits: i32) {
    validate_codec_options(opts, min_window_bits, true);
}

/// Validate once and retain the dictionary selected by that property read.
#[no_mangle]
pub extern "C" fn js_zlib_deflate_options(opts: f64, min_window_bits: i32) -> f64 {
    validate_codec_options(opts, min_window_bits, false)
}

fn validate_codec_options(opts: f64, min_window_bits: i32, warn_dictionary: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let jv = crate::value::JSValue::from_bits(opts.get_nanbox_f64().to_bits());
    if !jv.is_pointer() {
        return f64::from_bits(crate::value::JSValue::undefined().bits());
    }
    let ptr = jv.as_pointer::<crate::object::ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return f64::from_bits(crate::value::JSValue::undefined().bits());
    }
    // Each property read may allocate or invoke a getter. Recover the current
    // address from the root rather than carrying a pointer across those reads.
    let options_ptr = || {
        crate::value::JSValue::from_bits(opts.get_nanbox_f64().to_bits())
            .as_pointer::<crate::object::ObjectHeader>()
    };

    validate_option_range(
        options_ptr(),
        b"windowBits",
        "options.windowBits",
        // Policy zero permits exactly zero; all nonzero values still use 8..15.
        if min_window_bits == 0 {
            8
        } else {
            min_window_bits as i64
        },
        15,
        min_window_bits == 0,
    );
    validate_option_range(options_ptr(), b"level", "options.level", -1, 9, false);
    validate_option_range(options_ptr(), b"memLevel", "options.memLevel", 1, 9, false);
    validate_option_range(options_ptr(), b"strategy", "options.strategy", 0, 4, false);
    validate_option_chunk_size(read_option_field(options_ptr(), b"chunkSize"));
    validate_option_range(options_ptr(), b"flush", "options.flush", 0, 5, false);

    // Dictionary-aware consumers retain this exact validated read for their backend.
    // Other consumers keep their existing accepted-but-unapplied warning.
    let dict = scope.root_nanbox_f64(read_option_field(options_ptr(), b"dictionary"));
    let value = dict.get_nanbox_f64();
    if !crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        validate_dictionary_value(dict.get_nanbox_f64());
        if warn_dictionary {
            crate::stub_diag::perry_stub_warn(
                "zlib options.dictionary",
                "the preset dictionary option is accepted but not applied",
                Some("#4917"),
            );
        }
    }
    dict.get_nanbox_f64()
}

/// Zstd one-shot calls previously ignored other codec options. Read only the
/// dictionary here; do not impose DEFLATE ranges or extra property reads.
#[no_mangle]
pub extern "C" fn js_zlib_zstd_dictionary(opts: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let dictionary = scope.root_nanbox_f64(gzip_read_option(opts.get_nanbox_f64(), b"dictionary"));
    validate_dictionary_value(dictionary.get_nanbox_f64());
    dictionary.get_nanbox_f64()
}

fn validate_dictionary_value(value: f64) {
    if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        return;
    }
    // Public byte brands, not every cell backed by BufferHeader storage.
    // Dictionaries accept ArrayBuffer too; CRC32 intentionally does not.
    if crate::object::view_brand::view_brand(value).is_none()
        && crate::buffer::buffer_brand(crate::value::addr_class::object_ref_addr(value))
            != Some(crate::buffer::BufferBrand::ArrayBuffer)
    {
        crate::validators::throw_invalid_arg_type(
            "options.dictionary",
            "an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
            value,
        );
    }
}

// Gzip's constructor phases are separate from the shared one-shot validators.
// Each read recovers the current options pointer: getters may reenter and GC.
fn gzip_read_option(opts: f64, name: &[u8]) -> f64 {
    let value = crate::value::JSValue::from_bits(opts.to_bits());
    if !value.is_pointer()
        || !crate::value::addr_class::is_above_handle_band(
            crate::value::addr_class::object_ref_addr(opts),
        )
    {
        return f64::from_bits(crate::value::JSValue::undefined().bits());
    }
    read_option_field(value.as_pointer::<crate::object::ObjectHeader>(), name)
}

fn gzip_range(value: f64, name: &str, min: i64, max: i64, default: f64) -> f64 {
    let Some(n) = option_number(value, name) else {
        return default;
    };
    if n.is_nan() {
        return default;
    }
    if n < min as f64 || n > max as f64 {
        let received = crate::fs::validate::format_received_number(n);
        crate::fs::validate::throw_range_error_with_code(&format!(
            "The value of \"{name}\" is out of range. It must be >= {min} and <= {max}. Received {received}"
        ));
    }
    n
}

/// Resolve only Brotli one-shot output policy; do not reorder shared validators.
#[no_mangle]
pub extern "C" fn js_zlib_brotli_max_output_length(opts: f64) -> usize {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    gzip_range(
        gzip_read_option(opts.get_nanbox_f64(), b"maxOutputLength"),
        "options.maxOutputLength",
        1,
        9007199254740991,
        9007199254740991.0,
    ) as usize
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_BROTLI_MAX_OUTPUT_LENGTH: extern "C" fn(f64) -> usize =
    js_zlib_brotli_max_output_length;
// The bulk birth owns its callback-free allocation window. All heap-valued
// entries must be rooted by the caller until the returned record is rooted.
fn gzip_option_record(entries: &[(&str, f64)]) -> f64 {
    let keys: Vec<_> = entries.iter().map(|(key, _)| key.as_ptr()).collect();
    let lengths: Vec<_> = entries.iter().map(|(key, _)| key.len()).collect();
    let values: Vec<_> = entries.iter().map(|(_, value)| *value).collect();
    unsafe {
        crate::value::js_nanbox_pointer(crate::object::js_object_alloc_null_proto_entries(
            keys.as_ptr(),
            lengths.as_ptr(),
            values.as_ptr(),
            entries.len() as u32,
        ) as i64)
    }
}

/// Sync DEFLATE captures codec values before the real Transform spread.
/// The caller copies the selected dictionary before any later getter runs.
#[no_mangle]
pub extern "C" fn js_zlib_sync_codec_options(opts: f64, min_window_bits: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let read = |key| gzip_read_option(opts.get_nanbox_f64(), key);
    let first = crate::value::JSValue::from_bits(read(b"windowBits").to_bits());
    if !first.is_null() && !first.is_undefined() {
        read(b"windowBits");
    }
    let window_value = read(b"windowBits");
    let window =
        if min_window_bits == 0 && option_number(window_value, "options.windowBits") == Some(0.0) {
            0.0
        } else {
            gzip_range(window_value, "options.windowBits", 8, 15, 15.0)
        };
    let level = gzip_range(read(b"level"), "options.level", -1, 9, -1.0);
    let mem = gzip_range(read(b"memLevel"), "options.memLevel", 1, 9, 8.0);
    let strategy = gzip_range(read(b"strategy"), "options.strategy", 0, 4, 0.0);
    let dict = scope.root_nanbox_f64(read(b"dictionary"));
    let value = dict.get_nanbox_f64();
    if !crate::value::JSValue::from_bits(value.to_bits()).is_undefined()
        && crate::object::view_brand::view_brand(value).is_none()
        && crate::buffer::buffer_brand(crate::value::addr_class::object_ref_addr(value))
            != Some(crate::buffer::BufferBrand::ArrayBuffer)
    {
        crate::validators::throw_invalid_arg_type(
            "options.dictionary",
            "an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
            dict.get_nanbox_f64(),
        );
    }
    gzip_option_record(&[
        ("windowBits", window),
        ("level", level),
        ("memLevel", mem),
        ("strategy", strategy),
        ("dictionary", dict.get_nanbox_f64()),
    ])
}

/// Complete the actual option spread, then retain the last info read once.
/// Internal records contain data fields only; copying them cannot invoke JS.
#[no_mangle]
pub extern "C" fn js_zlib_sync_finish_options(opts: f64, codec: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let codec = scope.root_nanbox_f64(codec);
    let base = scope.root_nanbox_f64(js_zlib_gzip_base_options(opts.get_nanbox_f64()));
    let source = scope.root_nanbox_f64(js_zlib_gzip_transform_options(opts.get_nanbox_f64(), 0));
    let copy = scope.root_nanbox_f64(js_zlib_gzip_transform_options(source.get_nanbox_f64(), 1));
    let info = scope.root_nanbox_f64(gzip_read_option(source.get_nanbox_f64(), b"info"));
    let selected = scope.root_nanbox_f64(gzip_option_record(&[
        ("info", info.get_nanbox_f64()),
        (
            "_syncInfo",
            f64::from_bits(
                crate::value::JSValue::bool(crate::value::js_is_truthy(info.get_nanbox_f64()) != 0)
                    .bits(),
            ),
        ),
    ]));
    for record in [&codec, &base, &selected] {
        unsafe {
            crate::object::js_object_literal_spread(copy.get_nanbox_f64(), record.get_nanbox_f64());
        }
    }
    copy.get_nanbox_f64()
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_SYNC_CODEC_OPTIONS: extern "C" fn(f64, i32) -> f64 = js_zlib_sync_codec_options;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_SYNC_FINISH_OPTIONS: extern "C" fn(f64, f64) -> f64 =
    js_zlib_sync_finish_options;

/// Capture FIRST validated codec values before native codec initialization.
/// The returned traced record also retains the selected dictionary; accepting
/// it still does not apply a preset dictionary to the compression backend.
#[no_mangle]
pub extern "C" fn js_zlib_gzip_codec_options(opts: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let read = |key| gzip_read_option(opts.get_nanbox_f64(), key);
    let first = crate::value::JSValue::from_bits(read(b"windowBits").to_bits());
    // Even for Gzip, JS evaluates the second operand of the first OR when
    // the first value is not nullish, before testing the decompressor modes.
    if !first.is_null() && !first.is_undefined() {
        read(b"windowBits");
    }
    let window = gzip_range(read(b"windowBits"), "options.windowBits", 9, 15, 15.0);
    let level = gzip_range(read(b"level"), "options.level", -1, 9, -1.0);
    let mem_level = gzip_range(read(b"memLevel"), "options.memLevel", 1, 9, 8.0);
    let strategy = gzip_range(read(b"strategy"), "options.strategy", 0, 4, 0.0);
    let dictionary = scope.root_nanbox_f64(read(b"dictionary"));
    let value = dictionary.get_nanbox_f64();
    if !crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        if crate::object::view_brand::view_brand(value).is_none()
            && crate::buffer::buffer_brand(crate::value::addr_class::object_ref_addr(value))
                != Some(crate::buffer::BufferBrand::ArrayBuffer)
        {
            crate::validators::throw_invalid_arg_type(
                "options.dictionary",
                "an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
                dictionary.get_nanbox_f64(),
            );
        }
        crate::stub_diag::perry_stub_warn(
            "zlib options.dictionary",
            "the preset dictionary option is accepted but not applied",
            Some("#4917"),
        );
    }
    gzip_option_record(&[
        ("windowBits", window),
        ("level", level),
        ("memLevel", mem_level),
        ("strategy", strategy),
        ("dictionary", dictionary.get_nanbox_f64()),
    ])
}

/// Enter ZlibBase only AFTER the same codec has a traced engine owner.
#[no_mangle]
pub extern "C" fn js_zlib_gzip_base_options(opts: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let read = |key| gzip_read_option(opts.get_nanbox_f64(), key);
    // handle.init reads this argument before ZlibBase validates its type.
    read(b"rejectGarbageAfterEnd");
    let chunk = validate_option_chunk_size(read(b"chunkSize"));
    let flush = gzip_range(read(b"flush"), "options.flush", 0, 5, 0.0);
    let finish = gzip_range(read(b"finishFlush"), "options.finishFlush", 0, 5, 4.0);
    let max = gzip_range(
        read(b"maxOutputLength"),
        "options.maxOutputLength",
        1,
        9007199254740991,
        9007199254740991.0,
    );
    if !crate::value::JSValue::from_bits(read(b"rejectGarbageAfterEnd").to_bits()).is_undefined() {
        crate::validators::validate_boolean(
            read(b"rejectGarbageAfterEnd"),
            "options.rejectGarbageAfterEnd",
        );
    }
    gzip_option_record(&[
        ("chunkSize", chunk),
        ("flush", flush),
        ("finishFlush", finish),
        ("maxOutputLength", max),
    ])
}

/// The actual ZlibBase option normalization, followed by the actual Transform
/// spread. No fixed getter schedule or cached simulation of CopyDataProperties.
/// Phase zero returns the (possibly normalized) source for post-Transform reads;
/// phase one returns the real `{ autoDestroy: true, ...opts }` argument.
#[no_mangle]
pub extern "C" fn js_zlib_gzip_transform_options(opts: f64, phase: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    if phase == 0 {
        let read = |key| gzip_read_option(opts.get_nanbox_f64(), key);
        if crate::value::js_is_truthy(read(b"encoding")) == 0
            && crate::value::js_is_truthy(read(b"objectMode")) == 0
            && crate::value::js_is_truthy(read(b"writableObjectMode")) == 0
            && read(b"decodeStrings").to_bits() != crate::value::JSValue::bool(false).bits()
        {
            return opts.get_nanbox_f64();
        }
        let copy = scope.root_nanbox_f64(gzip_option_record(&[]));
        unsafe {
            crate::object::js_object_literal_spread(copy.get_nanbox_f64(), opts.get_nanbox_f64());
        }
        // Null-prototype target: these data writes cannot invoke inherited JS.
        for (key, value) in [
            (
                "encoding",
                f64::from_bits(crate::value::JSValue::null().bits()),
            ),
            (
                "objectMode",
                f64::from_bits(crate::value::JSValue::bool(false).bits()),
            ),
            (
                "writableObjectMode",
                f64::from_bits(crate::value::JSValue::bool(false).bits()),
            ),
            (
                "decodeStrings",
                f64::from_bits(crate::value::JSValue::bool(true).bits()),
            ),
        ] {
            let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
                key.as_ptr(),
                key.len() as u32,
            ));
            key.with_const_ptr(|key: *const crate::StringHeader| unsafe {
                crate::object::js_object_set_property_key(
                    copy.get_nanbox_f64(),
                    crate::value::js_nanbox_string(key as i64),
                    value,
                )
            });
        }
        return copy.get_nanbox_f64();
    }
    let copy = scope.root_nanbox_f64(gzip_option_record(&[(
        "autoDestroy",
        f64::from_bits(crate::value::JSValue::bool(true).bits()),
    )]));
    unsafe {
        crate::object::js_object_literal_spread(copy.get_nanbox_f64(), opts.get_nanbox_f64());
    }
    copy.get_nanbox_f64()
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_GZIP_CODEC_OPTIONS: extern "C" fn(f64) -> f64 = js_zlib_gzip_codec_options;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_GZIP_BASE_OPTIONS: extern "C" fn(f64) -> f64 = js_zlib_gzip_base_options;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_GZIP_TRANSFORM_OPTIONS: extern "C" fn(f64, i32) -> f64 =
    js_zlib_gzip_transform_options;

/// Validate the `buffer` argument to a one-shot zlib codec (`gzipSync`,
/// `deflateSync`, `gunzipSync`, `inflateSync`, …) the way Node does (#3662):
/// the value must be a string or an instance of Buffer/TypedArray/DataView/
/// ArrayBuffer, otherwise `TypeError [ERR_INVALID_ARG_TYPE]` is thrown.
///
/// `data_bits` is the raw NaN-box bit pattern of the data argument (the codecs
/// receive it as a full value via `NA_F64`). The in-tree `perry-stdlib` codecs
/// already perform this check inside `codec_bytes`; this shared helper lets the
/// out-of-tree `perry-ext-zlib` codecs (used by the auto-optimize build) reject
/// the same inputs without duplicating the runtime's value typing.
#[no_mangle]
pub extern "C" fn js_zlib_validate_buffer_arg(data_bits: i64) {
    let value = f64::from_bits(data_bits as u64);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return;
    }
    // Buffer / TypedArray / DataView / ArrayBuffer all live behind a heap
    // pointer; recover the raw address (pointer/string-tagged values mask off
    // the tag, a bare small pointer is used as-is) and probe the registries.
    // #10694: the probes below read the cell's header, so a raw (top16 == 0)
    // word must be one the allocator owns; `object_ref_addr` vouches for it.
    let raw = crate::value::addr_class::object_ref_addr(value);
    if raw >= 0x1000 {
        if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
            return;
        }
        if crate::buffer::is_registered_buffer(raw) {
            return;
        }
    }
    let received = crate::fs::validate::describe_received(value);
    let message = format!(
        "The \"buffer\" argument must be of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer. Received {received}"
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Validate CRC32's data before its optional uint32 seed, without copying data.
/// Unlike the other codecs, CRC32 accepts views but not an ArrayBuffer itself.
#[no_mangle]
pub extern "C" fn js_zlib_validate_crc32_args(data: f64, seed: f64) -> u32 {
    // CRC32 accepts public view brands, not every BufferHeader storage cell.
    if !crate::value::JSValue::from_bits(data.to_bits()).is_any_string()
        && crate::object::view_brand::view_brand(data).is_none()
    {
        crate::validators::throw_invalid_arg_type(
            "data",
            "of type string or an instance of Buffer, TypedArray, or DataView",
            data,
        );
    }
    if crate::value::JSValue::from_bits(seed.to_bits()).is_undefined() {
        0
    } else {
        crate::validators::validate_uint32(seed, "value")
    }
}

/// Validate the required callback for async one-shot zlib helpers
/// (`gzip(data, cb)`, `gunzip(data, cb)`, `brotliDecompress(data, cb)`, ...).
/// Node throws synchronously before queuing codec work when the callback is
/// missing or not callable.
#[no_mangle]
pub extern "C" fn js_zlib_validate_callback(callback: f64) -> i64 {
    crate::fs::validate::validate_required_callback("callback", callback) as i64
}

/// Keep the codegen-emitted symbol alive through the whole-program LLVM
/// bitcode rebuild performed by auto-optimize (see
/// `project_auto_optimize_keepalive_3320`). Called only from generated `.o` /
/// `perry-ext-zlib`, so without an explicit anchor the dead-stripper drops it.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_RESOLVE_LEVEL: extern "C" fn(f64) -> i32 = js_zlib_resolve_level;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_VALIDATE_PARAMS: extern "C" fn(f64, f64) -> i32 = js_zlib_validate_params;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_VALIDATE_OPTIONS: extern "C" fn(f64, i32) = js_zlib_validate_options;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_DEFLATE_OPTIONS: extern "C" fn(f64, i32) -> f64 = js_zlib_deflate_options;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_VALIDATE_BUFFER_ARG: extern "C" fn(i64) = js_zlib_validate_buffer_arg;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_VALIDATE_CRC32_ARGS: extern "C" fn(f64, f64) -> u32 =
    js_zlib_validate_crc32_args;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_VALIDATE_CALLBACK: extern "C" fn(f64) -> i64 = js_zlib_validate_callback;

/// Read a stream sizing option before entering the external codec's mutex.
/// This helper also validates the Readable/Writable high-water marks.
#[no_mangle]
pub extern "C" fn js_zlib_stream_option(opts: f64, which: i32) -> usize {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_nanbox_f64(opts);
    let field: &[u8] = match which {
        0 => b"chunkSize",
        1 => b"readableHighWaterMark",
        _ => b"writableHighWaterMark",
    };
    let default = if which == 0 { 16384 } else { 65536 };
    let value = crate::value::JSValue::from_bits(opts.get_nanbox_f64().to_bits());
    if !value.is_pointer() {
        return default;
    }
    if !crate::value::addr_class::is_above_handle_band(crate::value::addr_class::object_ref_addr(
        opts.get_nanbox_f64(),
    )) {
        return default;
    }
    let read = |name| {
        let live = crate::value::JSValue::from_bits(opts.get_nanbox_f64().to_bits());
        read_option_field(live.as_pointer::<crate::object::ObjectHeader>(), name)
    };
    if which == 0 {
        return validate_option_chunk_size(read(field)) as usize;
    }
    let mut v = read(field);
    if which != 0 {
        let common = read(b"highWaterMark");
        let common_value = crate::value::JSValue::from_bits(common.to_bits());
        if !common_value.is_undefined() && !common_value.is_null() {
            v = common;
        }
    }
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return default;
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        v
    };
    if !jv.is_number() && !jv.is_int32()
        || !n.is_finite()
        || n < (if which == 0 { 64.0 } else { 0.0 })
        || (which != 0 && n.fract() != 0.0)
    {
        crate::fs::validate::throw_type_error_with_code(
            &format!(
                "The property 'options.{}' is invalid",
                std::str::from_utf8(field).unwrap()
            ),
            "ERR_INVALID_ARG_VALUE",
        );
    }
    n as usize
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_STREAM_OPTION: extern "C" fn(f64, i32) -> usize = js_zlib_stream_option;

#[no_mangle]
pub unsafe extern "C" fn js_zlib_stream_error(
    message: *const u8,
    len: usize,
    truncated: i32,
) -> f64 {
    let text = crate::string::js_string_from_bytes(message, len as u32);
    crate::node_submodules::register_error_code_pub(
        text,
        match truncated {
            1 => "Z_BUF_ERROR",
            2 => "ERR_STREAM_DESTROYED",
            _ => "Z_DATA_ERROR",
        },
    );
    crate::value::js_nanbox_pointer(crate::error::js_error_new_with_message(text) as i64)
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_STREAM_ERROR: unsafe extern "C" fn(*const u8, usize, i32) -> f64 =
    js_zlib_stream_error;

#[no_mangle]
pub extern "C" fn js_zlib_is_callback(value: f64) -> i32 {
    let addr = crate::value::addr_class::object_ref_addr(value);
    (crate::value::addr_class::is_above_handle_band(addr)
        && !crate::closure::get_valid_func_ptr(addr as *const crate::closure::ClosureHeader)
            .is_null()) as i32
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_ZLIB_IS_CALLBACK: extern "C" fn(f64) -> i32 = js_zlib_is_callback;
