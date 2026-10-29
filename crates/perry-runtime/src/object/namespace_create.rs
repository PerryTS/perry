//! `js_create_namespace` (Issue #100): build a module-namespace object from
//! parallel key/value arrays. Split out of `object_ops.rs` to keep that
//! module under the file-size gate.

use super::*;

pub(crate) const MODULE_NAMESPACE_CLASS_ID: u32 = 0xFFFF_4E53;

/// #9889: Implement the module namespace exotic object's [[GetOwnProperty]]
/// result while retaining accessors as the internal representation of live
/// bindings. The getter can allocate and evacuate both the namespace and key,
/// so keep them rooted until it returns. `build_data_descriptor` roots the
/// resulting value before allocating the descriptor object.
pub(crate) unsafe fn module_namespace_own_property_descriptor(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let key_handle = scope.root_string_ptr(key);
    let value = obj_handle.with_mut_ptr::<ObjectHeader, _>(|current_obj| {
        key_handle.with_const_ptr::<crate::StringHeader, _>(|current_key| {
            js_object_get_field_by_name(current_obj, current_key)
        })
    });
    super::descriptors::build_data_descriptor(f64::from_bits(value.bits()), true, true, false)
}

/// Apply the host-defined invariants shared by static and dynamic module
/// namespace objects.
#[no_mangle]
pub extern "C" fn js_finalize_namespace(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return value;
    }
    let obj = jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if obj.is_null()
        || !crate::value::addr_class::is_above_handle_band(obj as usize)
        || !crate::object::is_valid_obj_ptr(obj as *const u8)
    {
        return value;
    }
    unsafe {
        (*obj).class_id = MODULE_NAMESPACE_CLASS_ID;
        // A module namespace exotic object has a [[Prototype]] of `null`
        // (ECMA-262 10.4.6), and `OBJ_FLAG_NULL_PROTO` is the proof
        // `Object.getPrototypeOf` already reads to answer `null`. Without it a
        // namespace reached the self-prototype fallback in
        // `object_ops/prototype.rs`, which answers with a further namespace
        // object, so a prototype-chain walk over an `import * as ns` binding
        // never terminated. That is the same non-termination the fallback's
        // `NATIVE_MODULE_CLASS_ID` arm already guards against for
        // `require("path")` namespaces; a static ESM namespace reaches this
        // function instead, and had no such guard.
        let gc = crate::gc::header_from_trusted_user_ptr(obj.cast()).cast_mut();
        (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
        crate::object::shapes::restamp_object_proto_id(obj);
    }
    crate::object::js_object_prevent_extensions(value)
}

/// Issue #100: build a module-namespace object (the value an `await
/// import("./foo.ts")` resolves to) from parallel arrays of keys and
/// values. Entries whose parallel `live_flags` byte is non-zero carry a
/// zero-argument getter closure instead of a snapshot value.
///
/// Keys are length-prefixed UTF-8 (Perry strings are not guaranteed
/// null-terminated), passed as parallel `*const *const u8` (data
/// pointers) and `*const i32` (byte lengths). Values are the already
/// NaN-boxed `f64` representations passed as a flat `f64` array.
///
/// The returned f64 is a NaN-boxed POINTER_TAG `ObjectHeader` with its
/// `keys_array` populated so `Object.keys(ns)`/iteration and property
/// dispatch work the same as on any other JS object. Caller is
/// responsible for pinning the object as a GC root if it stores the
/// result in a long-lived slot — codegen does this by writing the
/// result into the module-scoped `__perry_ns_<prefix>` global which is
/// already registered with `js_gc_register_global_root`.
///
/// Empty namespace (`n == 0`) returns a fresh empty object.
///
/// Returns an `f64` directly (not `JSValue`) so the LLVM ABI signature
/// `double js_create_namespace(...)` declared in `runtime_decls.rs`
/// matches: NaN-boxed values use float-register-return on AArch64 /
/// SysV-x86_64. A `JSValue` return would route through integer
/// registers (`#[repr(transparent)]` over `u64`) and the call site's
/// `%xmm0` read would observe stale bits.
#[no_mangle]
pub extern "C" fn js_create_namespace(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
    live_flags: *const u8,
) -> f64 {
    js_finalize_namespace(build_namespace_object(
        n, keys, key_lens, values, live_flags,
    ))
}

/// The namespace of a CommonJS module under Bun's interop: the static
/// entries (the lexer-detected names, `default`, `module.exports`), plus
/// every other own enumerable string key of the evaluated `module.exports`
/// object. Bun exposes all of them on `import * as ns`; the static lexer
/// cannot see names that only exist at run time (a UMD factory's return
/// value, `module.exports = make()`), so they are read here, once, after
/// the module body ran. Values are a snapshot, like Node's detected names.
/// `default` already holds the exports object and `__esModule` is interop
/// metadata, so neither is copied.
#[no_mangle]
pub extern "C" fn js_create_cjs_namespace(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
    live_flags: *const u8,
) -> f64 {
    let ns = build_namespace_object(n, keys, key_lens, values, live_flags);
    js_finalize_namespace(js_namespace_adopt_cjs_exports(ns))
}

/// The consumer-side twin of [`js_create_cjs_namespace`]: an `import * as ns`
/// of a CommonJS module read as a whole value is materialized from the
/// importer's member list, then takes the run-time `module.exports` keys here
/// before it is finalized. Returns the (possibly relocated) namespace.
#[no_mangle]
pub extern "C" fn js_namespace_adopt_cjs_exports(ns: f64) -> f64 {
    unsafe { adopt_cjs_exports(ns) }
}

/// Copy the own enumerable string keys of `ns.default` (the evaluated
/// `module.exports`) that `ns` does not already own. `ns` is not finalized
/// yet, so it is still extensible. Everything that can allocate keeps the
/// namespace, the copy, and the current key rooted.
unsafe fn adopt_cjs_exports(ns: f64) -> f64 {
    let ns_v = JSValue::from_bits(ns.to_bits());
    if !ns_v.is_pointer() {
        return ns;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ns_h = scope.root_raw_mut_ptr(ns_v.as_pointer::<ObjectHeader>() as *mut ObjectHeader);
    let default_key = crate::string::js_string_from_bytes(b"default".as_ptr(), 7);
    let exports = ns_h
        .with_mut_ptr::<ObjectHeader, _>(|obj| js_object_get_field_by_name_f64(obj, default_key));
    let current = |h: &crate::gc::RuntimeHandle<'_>| {
        h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64))
    };
    if !JSValue::from_bits(exports.to_bits()).is_reference() {
        return current(&ns_h);
    }
    let exports_h = scope.root_nanbox_f64(exports);
    // CopyDataProperties of the exports value (objects, functions and
    // proxies alike) into a plain object, then move the missing keys over.
    let staging_obj = js_object_alloc(0, 0);
    let staging_h = scope.root_raw_mut_ptr(staging_obj);
    let staging_v =
        staging_h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
    super::assign::js_object_literal_spread(staging_v, exports_h.get_nanbox_f64());
    let staging_v =
        staging_h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
    let names = js_object_keys_value(staging_v);
    let names_h = scope.root_raw_mut_ptr(names);
    let len = crate::array::js_array_length(names);
    for i in 0..len {
        let iter_scope = crate::gc::RuntimeHandleScope::new();
        let key_v = names_h
            .with_mut_ptr::<crate::array::ArrayHeader, _>(|a| crate::array::js_array_get_f64(a, i));
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key_v) as *const crate::StringHeader;
        if key_ptr.is_null() {
            continue;
        }
        match super::has_own_helpers::str_from_string_header(key_ptr) {
            Some("default") | Some("__esModule") | None => continue,
            Some(_) => {}
        }
        let key_h = iter_scope.root_string_ptr(key_ptr);
        let ns_value =
            ns_h.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64));
        let key_value = key_h
            .with_const_ptr::<crate::StringHeader, _>(|k| crate::value::js_nanbox_string(k as i64));
        if crate::value::js_is_truthy(js_object_has_own(ns_value, key_value)) != 0 {
            continue;
        }
        let value = staging_h.with_mut_ptr::<ObjectHeader, _>(|o| {
            key_h
                .with_const_ptr::<crate::StringHeader, _>(|k| js_object_get_field_by_name_f64(o, k))
        });
        let value_h = iter_scope.root_nanbox_f64(value);
        ns_h.with_mut_ptr::<ObjectHeader, _>(|o| {
            key_h.with_const_ptr::<crate::StringHeader, _>(|k| {
                js_object_set_field_by_name(o, k, value_h.get_nanbox_f64())
            })
        });
    }
    current(&ns_h)
}

fn build_namespace_object(
    n: i32,
    keys: *const *const u8,
    key_lens: *const i32,
    values: *const f64,
    live_flags: *const u8,
) -> f64 {
    let count = if n < 0 { 0 } else { n as usize };
    unsafe {
        // Export values arrive in a caller stack buffer, which is not part of
        // the runtime root set. Namespace construction allocates the object,
        // keys array, and key strings; retain every value across those moving
        // collections so CJS default/shared object identity is preserved.
        let raw_values = if count == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(values, count).to_vec()
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let value_handles = scope.root_nanbox_f64_slice(&raw_values);
        // Allocate a plain object with `count` inline slots. class_id 0
        // is the generic-object class used by Object.create / {} / URL.
        let obj = js_object_alloc(0, count as u32);
        if obj.is_null() {
            // Fallback to undefined — should never happen but defensive.
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let obj_handle = scope.root_raw_mut_ptr(obj);

        // Initialize an empty keys array so `js_object_set_field_by_name`
        // can append to it. Pre-populating the keys array AND calling
        // set_field_by_name would double every key — the property
        // setter's "add key to keys_array" step runs unconditionally.
        let keys_arr = crate::array::js_array_alloc(0);
        let mut obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        js_object_set_keys(obj, keys_arr);

        // Set each (key, value) pair on the object. We route through
        // `js_object_set_field_by_name` so the standard property-write
        // path (inline-slot allocation, shape transitions, accessor
        // dispatch) handles everything. This matches how user-written
        // `obj.k = v` and `js_object_assign_one` populate objects, so
        // downstream reads (PropertyGet PIC, Object.keys, JSON.stringify)
        // all work without special-casing the namespace shape.
        for i in 0..count {
            let key_data = *keys.add(i);
            let key_len = *key_lens.add(i);
            let key_len_u = if key_len < 0 { 0u32 } else { key_len as u32 };
            // Use the heap StringHeader path so the property machinery
            // (which expects a real `StringHeader*`) gets a valid
            // pointer. Pre-SSO-only would crash on >7-byte export names.
            let key_hdr = crate::string::js_string_from_bytes(key_data, key_len_u);
            obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
            let val = value_handles[i].get_nanbox_f64();
            if !live_flags.is_null() && *live_flags.add(i) != 0 {
                let key_handle = scope.root_string_ptr(key_hdr);
                obj_handle.with_mut_ptr::<ObjectHeader, _>(|current_obj| {
                    key_handle.with_const_ptr::<crate::StringHeader, _>(|current_key| {
                        js_object_define_accessor(
                            crate::value::js_nanbox_pointer(current_obj as i64),
                            crate::value::js_nanbox_string(current_key as i64),
                            val,
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        );
                    });
                });
                let key = String::from_utf8_lossy(std::slice::from_raw_parts(
                    key_data,
                    key_len_u as usize,
                ))
                .into_owned();
                obj_handle.with_mut_ptr::<ObjectHeader, _>(|current_obj| {
                    set_property_attrs(
                        current_obj as usize,
                        key,
                        PropertyAttrs::new(false, true, false),
                    );
                });
            } else {
                js_object_set_field_by_name(obj, key_hdr, val);
            }
        }

        // NaN-box POINTER_TAG and return.
        obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
        let bits = (obj as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }
}

/// A module namespace born in its compiler-owned final layout: `desc` is the
/// module's namespace descriptor (`perry-codegen` `layout_desc::
/// namespace_layout`): its export names in sorted order (ECMA-262 10.4.6,
/// [[OwnPropertyKeys]]), slot = position, each key's attributes (a live
/// binding is an accessor whose getter reads the binding; every other export
/// a data property), then `@@toStringTag`, a `null` [[Prototype]], and born
/// non-extensible. So the
/// object carries its final shape from its first instruction: no key-add,
/// attribute, prototype or integrity transition, and no shape is minted.
///
/// `values` holds one NaN-boxed value per key, in key order: the getter
/// closure of an accessor key, the value of a data key, and last the
/// `@@toStringTag` value "Module".
///
/// # Safety
/// `desc` is a registered namespace descriptor of this program and `values`
/// points at `desc.key_count` values.
#[no_mangle]
pub unsafe extern "C" fn js_create_namespace_layout(
    desc: *const super::layout_desc::PerryLayoutDesc,
    values: *const f64,
) -> f64 {
    use super::key_attrs::ENTRY_ACCESSOR;
    let d = &*desc;
    let count = d.key_count as usize;
    let keys = if count == 0 {
        &[][..]
    } else {
        std::slice::from_raw_parts(d.keys, count)
    };
    if super::layout_desc::register_namespace_layout(desc).is_none() {
        // This runtime could not register the layout: the same namespace,
        // built key by key from the descriptor's own names and attributes.
        return namespace_from_unregistered_layout(keys, values);
    }
    // The values arrive in a caller stack buffer the collector does not
    // scan, and each accessor's pair allocation can collect: root them all,
    // then each pair, before the object exists.
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw: Vec<f64> = if count == 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(values, count).to_vec()
    };
    let value_handles = scope.root_nanbox_f64_slice(&raw);
    let mut words = Vec::with_capacity(count);
    for (key, value) in keys.iter().zip(value_handles.iter()) {
        if key.attrs & ENTRY_ACCESSOR != 0 {
            let pair = super::accessor_pair::pair_new(super::accessor_pair::Accessor {
                get: value.get_nanbox_u64(),
                ..Default::default()
            });
            words.push(Some(scope.root_nanbox_u64(
                crate::value::js_nanbox_pointer(pair as i64).to_bits(),
            )));
        } else {
            words.push(None);
        }
    }
    // Nothing below collects: the birth and its slot stores are one step.
    let _no_gc = crate::gc::GcSuppressScope::new();
    let obj = crate::realm_decl! { birth super::layout_desc::perry_alloc_layout(desc); realm { "node:assert", members = { Assert; AssertionError = undefined; deepEqual; deepStrictEqual; default; doesNotMatch; doesNotReject; doesNotThrow; equal; fail; ifError; match; notDeepEqual; notDeepStrictEqual; notEqual; notStrictEqual; ok = ref("node:assert.default"); partialDeepStrictEqual; rejects; strict; strictEqual; throws; }, births = [Array, Error, Function, Object]; "node:assert/strict", members = { Assert = ref("node:assert.Assert"); AssertionError = undefined; deepEqual; deepStrictEqual; default; doesNotMatch; doesNotReject; doesNotThrow; equal; fail; ifError; match; notDeepEqual; notDeepStrictEqual; notEqual; notStrictEqual; ok = ref("node:assert/strict.default"); partialDeepStrictEqual; rejects; strict = ref("node:assert.strict"); strictEqual; throws; }, births = [Array, Error, Function, Object]; "node:async_hooks", members = { AsyncLocalStorage; AsyncResource; asyncWrapProviders; createHook; default; executionAsyncId; executionAsyncResource; triggerAsyncId; }, births = [Array, Error, Function, Object]; "node:buffer", members = { Blob = ref(Blob); Buffer = ref(Buffer); File = ref(File); INSPECT_MAX_BYTES = 50.0; atob; btoa; constants; default; isAscii; isUtf8; kMaxLength = 9007199254740991.0; kStringMaxLength = 536870888.0; resolveObjectURL = undefined; transcode = undefined; }, births = [Array, ArrayBuffer, Error, Function, Object, Promise, Uint8Array]; "node:child_process", members = { ChildProcess; _forkChild; default; exec; execFile; execFileSync; execSync; fork; spawn; spawnSync; }, births = [Array, Buffer, Error, Function, Object, Promise]; "node:cluster", members = { SCHED_NONE = 1.0; SCHED_RR = 2.0; Worker; default; disconnect; fork; isMaster = true; isPrimary = true; isWorker = false; schedulingPolicy = 2.0; settings; setupMaster; setupPrimary = ref("node:cluster.setupMaster"); workers; }, births = [Array, Error, Function, Object]; "node:console", members = { Console = ref(console.Console); assert = ref(console.assert); clear = ref(console.clear); context = ref(console.context); count = ref(console.count); countReset = ref(console.countReset); createTask = ref(console.createTask); debug = ref(console.debug); default; dir = ref(console.dir); dirxml = ref(console.dirxml); error = ref(console.error); group = ref(console.group); groupCollapsed = ref(console.groupCollapsed); groupEnd = ref(console.groupEnd); info = ref(console.info); log = ref(console.log); profile = ref(console.profile); profileEnd = ref(console.profileEnd); table = ref(console.table); time = ref(console.time); timeEnd = ref(console.timeEnd); timeLog = ref(console.timeLog); timeStamp = ref(console.timeStamp); trace = ref(console.trace); warn = ref(console.warn); }, births = [Array, Error, Function, Object]; "node:constants", members = { COPYFILE_EXCL = 1.0; COPYFILE_FICLONE = 2.0; COPYFILE_FICLONE_FORCE = 4.0; DH_CHECK_P_NOT_PRIME = 1.0; DH_CHECK_P_NOT_SAFE_PRIME = 2.0; DH_NOT_SUITABLE_GENERATOR = 8.0; DH_UNABLE_TO_CHECK_GENERATOR = 4.0; E2BIG = 7.0; EACCES = 13.0; EADDRINUSE = 98.0; EADDRNOTAVAIL = 99.0; EAFNOSUPPORT = 97.0; EAGAIN = 11.0; EALREADY = 114.0; EBADF = 9.0; EBADMSG = 74.0; EBUSY = 16.0; ECANCELED = 125.0; ECHILD = 10.0; ECONNABORTED = 103.0; ECONNREFUSED = 111.0; ECONNRESET = 104.0; EDEADLK = 35.0; EDESTADDRREQ = 89.0; EDOM = 33.0; EDQUOT = 122.0; EEXIST = 17.0; EFAULT = 14.0; EFBIG = 27.0; EHOSTUNREACH = 113.0; EIDRM = 43.0; EILSEQ = 84.0; EINPROGRESS = 115.0; EINTR = 4.0; EINVAL = 22.0; EIO = 5.0; EISCONN = 106.0; EISDIR = 21.0; ELOOP = 40.0; EMFILE = 24.0; EMLINK = 31.0; EMSGSIZE = 90.0; EMULTIHOP = 72.0; ENAMETOOLONG = 36.0; ENETDOWN = 100.0; ENETRESET = 102.0; ENETUNREACH = 101.0; ENFILE = 23.0; ENGINE_METHOD_ALL = 65535.0; ENGINE_METHOD_CIPHERS = 64.0; ENGINE_METHOD_DH = 4.0; ENGINE_METHOD_DIGESTS = 128.0; ENGINE_METHOD_DSA = 2.0; ENGINE_METHOD_EC = 2048.0; ENGINE_METHOD_NONE = 0.0; ENGINE_METHOD_PKEY_ASN1_METHS = 1024.0; ENGINE_METHOD_PKEY_METHS = 512.0; ENGINE_METHOD_RAND = 8.0; ENGINE_METHOD_RSA = 1.0; ENOBUFS = 105.0; ENODATA = 61.0; ENODEV = 19.0; ENOENT = 2.0; ENOEXEC = 8.0; ENOLCK = 37.0; ENOLINK = 67.0; ENOMEM = 12.0; ENOMSG = 42.0; ENOPROTOOPT = 92.0; ENOSPC = 28.0; ENOSR = 63.0; ENOSTR = 60.0; ENOSYS = 38.0; ENOTCONN = 107.0; ENOTDIR = 20.0; ENOTEMPTY = 39.0; ENOTSOCK = 88.0; ENOTSUP = 95.0; ENOTTY = 25.0; ENXIO = 6.0; EOPNOTSUPP = 95.0; EOVERFLOW = 75.0; EPERM = 1.0; EPIPE = 32.0; EPROTO = 71.0; EPROTONOSUPPORT = 93.0; EPROTOTYPE = 91.0; ERANGE = 34.0; EROFS = 30.0; ESPIPE = 29.0; ESRCH = 3.0; ESTALE = 116.0; ETIME = 62.0; ETIMEDOUT = 110.0; ETXTBSY = 26.0; EWOULDBLOCK = 11.0; EXDEV = 18.0; F_OK = 0.0; OPENSSL_VERSION_NUMBER = 811597840.0; O_APPEND = 1024.0; O_CREAT = 64.0; O_DIRECT = 16384.0; O_DIRECTORY = 65536.0; O_DSYNC = 4096.0; O_EXCL = 128.0; O_NOATIME = 262144.0; O_NOCTTY = 256.0; O_NOFOLLOW = 131072.0; O_NONBLOCK = 2048.0; O_RDONLY = 0.0; O_RDWR = 2.0; O_SYMLINK = undefined; O_SYNC = 1052672.0; O_TRUNC = 512.0; O_WRONLY = 1.0; POINT_CONVERSION_COMPRESSED = 2.0; POINT_CONVERSION_HYBRID = 6.0; POINT_CONVERSION_UNCOMPRESSED = 4.0; PRIORITY_ABOVE_NORMAL = -7.0; PRIORITY_BELOW_NORMAL = 10.0; PRIORITY_HIGH = -14.0; PRIORITY_HIGHEST = -20.0; PRIORITY_LOW = 19.0; PRIORITY_NORMAL = 0.0; RSA_NO_PADDING = 3.0; RSA_PKCS1_OAEP_PADDING = 4.0; RSA_PKCS1_PADDING = 1.0; RSA_PKCS1_PSS_PADDING = 6.0; RSA_PSS_SALTLEN_AUTO = -2.0; RSA_PSS_SALTLEN_DIGEST = -1.0; RSA_PSS_SALTLEN_MAX_SIGN = -2.0; RSA_X931_PADDING = 5.0; RTLD_DEEPBIND = 8.0; RTLD_GLOBAL = 256.0; RTLD_LAZY = 1.0; RTLD_LOCAL = 0.0; RTLD_NOW = 2.0; R_OK = 4.0; SIGABRT = 6.0; SIGALRM = 14.0; SIGBUS = 7.0; SIGCHLD = 17.0; SIGCONT = 18.0; SIGFPE = 8.0; SIGHUP = 1.0; SIGILL = 4.0; SIGINFO = undefined; SIGINT = 2.0; SIGIO = 29.0; SIGIOT = 6.0; SIGKILL = 9.0; SIGPIPE = 13.0; SIGPOLL = 29.0; SIGPROF = 27.0; SIGPWR = 30.0; SIGQUIT = 3.0; SIGSEGV = 11.0; SIGSTKFLT = 16.0; SIGSTOP = 19.0; SIGSYS = 31.0; SIGTERM = 15.0; SIGTRAP = 5.0; SIGTSTP = 20.0; SIGTTIN = 21.0; SIGTTOU = 22.0; SIGURG = 23.0; SIGUSR1 = 10.0; SIGUSR2 = 12.0; SIGVTALRM = 26.0; SIGWINCH = 28.0; SIGXCPU = 24.0; SIGXFSZ = 25.0; SSL_OP_ALL = 2147485776.0; SSL_OP_ALLOW_NO_DHE_KEX = 1024.0; SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION = 262144.0; SSL_OP_CIPHER_SERVER_PREFERENCE = 4194304.0; SSL_OP_CISCO_ANYCONNECT = 32768.0; SSL_OP_COOKIE_EXCHANGE = 8192.0; SSL_OP_CRYPTOPRO_TLSEXT_BUG = 2147483648.0; SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS = 2048.0; SSL_OP_LEGACY_SERVER_CONNECT = 4.0; SSL_OP_NO_COMPRESSION = 131072.0; SSL_OP_NO_ENCRYPT_THEN_MAC = 524288.0; SSL_OP_NO_QUERY_MTU = 4096.0; SSL_OP_NO_RENEGOTIATION = 1073741824.0; SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION = 65536.0; SSL_OP_NO_SSLv2 = 0.0; SSL_OP_NO_SSLv3 = 33554432.0; SSL_OP_NO_TICKET = 16384.0; SSL_OP_NO_TLSv1 = 67108864.0; SSL_OP_NO_TLSv1_1 = 268435456.0; SSL_OP_NO_TLSv1_2 = 134217728.0; SSL_OP_NO_TLSv1_3 = 536870912.0; SSL_OP_PRIORITIZE_CHACHA = 2097152.0; SSL_OP_TLS_ROLLBACK_BUG = 8388608.0; S_IFBLK = 24576.0; S_IFCHR = 8192.0; S_IFDIR = 16384.0; S_IFIFO = 4096.0; S_IFLNK = 40960.0; S_IFMT = 61440.0; S_IFREG = 32768.0; S_IFSOCK = 49152.0; S_IRGRP = 32.0; S_IROTH = 4.0; S_IRUSR = 256.0; S_IRWXG = 56.0; S_IRWXO = 7.0; S_IRWXU = 448.0; S_IWGRP = 16.0; S_IWOTH = 2.0; S_IWUSR = 128.0; S_IXGRP = 8.0; S_IXOTH = 1.0; S_IXUSR = 64.0; TLS1_1_VERSION = 770.0; TLS1_2_VERSION = 771.0; TLS1_3_VERSION = 772.0; TLS1_VERSION = 769.0; UV_DIRENT_BLOCK = 7.0; UV_DIRENT_CHAR = 6.0; UV_DIRENT_DIR = 2.0; UV_DIRENT_FIFO = 4.0; UV_DIRENT_FILE = 1.0; UV_DIRENT_LINK = 3.0; UV_DIRENT_SOCKET = 5.0; UV_DIRENT_UNKNOWN = 0.0; UV_FS_COPYFILE_EXCL = 1.0; UV_FS_COPYFILE_FICLONE = 2.0; UV_FS_COPYFILE_FICLONE_FORCE = 4.0; UV_FS_O_FILEMAP = 0.0; UV_FS_SYMLINK_DIR = 1.0; UV_FS_SYMLINK_JUNCTION = 2.0; W_OK = 2.0; X_OK = 1.0; default; defaultCoreCipherList = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"; }, births = [Array, Error, Function, Object]; "node:crypto", members = { Certificate; Cipheriv; Decipheriv; DiffieHellman; DiffieHellmanGroup; ECDH; Hash; Hmac; KeyObject; Sign; Verify; X509Certificate; argon2; argon2Sync; checkPrime; checkPrimeSync; constants; createCipheriv; createDecipheriv; createDiffieHellman; createDiffieHellmanGroup; createECDH; createHash; createHmac; createPrivateKey; createPublicKey; createSecretKey; createSign; createVerify; decapsulate; default; diffieHellman; encapsulate; generateKey; generateKeyPair; generateKeyPairSync; generateKeySync; generatePrime; generatePrimeSync; getCipherInfo; getCiphers; getCurves; getDiffieHellman; getFips; getHashes; getRandomValues; hash; hkdf; hkdfSync; pbkdf2; pbkdf2Sync; privateDecrypt; privateEncrypt; publicDecrypt; publicEncrypt; randomBytes; randomFill; randomFillSync; randomInt; randomUUID; scrypt; scryptSync; secureHeapUsed; setFips; sign; subtle; timingSafeEqual; verify; webcrypto; }, births = [Array, ArrayBuffer, Buffer, Crypto, CryptoKey, Error, Function, Object, Promise, SubtleCrypto, Uint8Array]; "node:dgram", members = { Socket; createSocket; default; }, births = [Array, Buffer, Error, Function, Object, Promise]; "node:dns", members = { ADDRCONFIG = 32.0; ADDRGETNETWORKPARAMS = "EADDRGETNETWORKPARAMS"; ALL = 16.0; BADFAMILY = "EBADFAMILY"; BADFLAGS = "EBADFLAGS"; BADHINTS = "EBADHINTS"; BADNAME = "EBADNAME"; BADQUERY = "EBADQUERY"; BADRESP = "EBADRESP"; BADSTR = "EBADSTR"; CANCELLED = "ECANCELLED"; CONNREFUSED = "ECONNREFUSED"; DESTRUCTION = "EDESTRUCTION"; EOF = "EOF"; FILE = "EFILE"; FORMERR = "EFORMERR"; LOADIPHLPAPI = "ELOADIPHLPAPI"; NODATA = "ENODATA"; NOMEM = "ENOMEM"; NONAME = "ENONAME"; NOTFOUND = "ENOTFOUND"; NOTIMP = "ENOTIMP"; NOTINITIALIZED = "ENOTINITIALIZED"; REFUSED = "EREFUSED"; Resolver; SERVFAIL = "ESERVFAIL"; TIMEOUT = "ETIMEOUT"; V4MAPPED = 8.0; default; getDefaultResultOrder; getServers; lookup; lookupService; promises; resolve; resolve4; resolve6; resolveAny; resolveCaa; resolveCname; resolveMx; resolveNaptr; resolveNs; resolvePtr; resolveSoa; resolveSrv; resolveTlsa; resolveTxt; reverse; setDefaultResultOrder; setServers; }, births = [Array, Error, Function, Object]; "node:dns/promises", members = { ADDRGETNETWORKPARAMS = "EADDRGETNETWORKPARAMS"; BADFAMILY = "EBADFAMILY"; BADFLAGS = "EBADFLAGS"; BADHINTS = "EBADHINTS"; BADNAME = "EBADNAME"; BADQUERY = "EBADQUERY"; BADRESP = "EBADRESP"; BADSTR = "EBADSTR"; CANCELLED = "ECANCELLED"; CONNREFUSED = "ECONNREFUSED"; DESTRUCTION = "EDESTRUCTION"; EOF = "EOF"; FILE = "EFILE"; FORMERR = "EFORMERR"; LOADIPHLPAPI = "ELOADIPHLPAPI"; NODATA = "ENODATA"; NOMEM = "ENOMEM"; NONAME = "ENONAME"; NOTFOUND = "ENOTFOUND"; NOTIMP = "ENOTIMP"; NOTINITIALIZED = "ENOTINITIALIZED"; REFUSED = "EREFUSED"; Resolver; SERVFAIL = "ESERVFAIL"; TIMEOUT = "ETIMEOUT"; default = ref("node:dns.promises"); getDefaultResultOrder; getServers; lookup; lookupService; resolve; resolve4; resolve6; resolveAny; resolveCaa; resolveCname; resolveMx; resolveNaptr; resolveNs; resolvePtr; resolveSoa; resolveSrv; resolveTlsa; resolveTxt; reverse; setDefaultResultOrder; setServers; }, births = [Array, Error, Function, Object]; "node:domain", members = { Domain; _stack; active = null; create; createDomain = ref("node:domain.create"); default; members = undefined; }, births = [Array, Error, Function, Object]; "node:events", members = { EventEmitter; EventEmitterAsyncResource; addAbortListener; captureRejectionSymbol = Symbol.for("nodejs.rejection"); captureRejections = false; default = ref("node:events.EventEmitter"); defaultMaxListeners = 10.0; errorMonitor = Symbol.for("events.errorMonitor"); getEventListeners; getMaxListeners; init; listenerCount; on; once; setMaxListeners; usingDomains = false; }, births = ["%AsyncIteratorPrototype%", Array, Error, Event, EventTarget, Function, Object, Promise]; "node:fs", members = { Dir; Dirent; FileReadStream; FileWriteStream; ReadStream = ref("node:fs.FileReadStream"); Stats; Utf8Stream; WriteStream = ref("node:fs.FileWriteStream"); _toUnixTimestamp; access; accessSync; appendFile; appendFileSync; chmod; chmodSync; chown; chownSync; close; closeSync; constants; copyFile; copyFileSync; cp; cpSync; createReadStream; createWriteStream; default; exists; existsSync; fchmod; fchmodSync; fchown; fchownSync; fdatasync; fdatasyncSync; fstat; fstatSync; fsync; fsyncSync; ftruncate; ftruncateSync; futimes; futimesSync; glob; globSync; lchmod = undefined; lchmodSync = undefined; lchown; lchownSync; link; linkSync; lstat; lstatSync; lutimes; lutimesSync; mkdir; mkdirSync; mkdtemp; mkdtempDisposableSync; mkdtempSync; open; openAsBlob; openSync; opendir; opendirSync; promises; read; readFile; readFileSync; readSync; readdir; readdirSync; readlink; readlinkSync; readv; readvSync; realpath; realpathSync; rename; renameSync; rm; rmSync; rmdir; rmdirSync; stat; statSync; statfs; statfsSync; symlink; symlinkSync; truncate; truncateSync; unlink; unlinkSync; unwatchFile; utimes; utimesSync; watch; watchFile; write; writeFile; writeFileSync; writeSync; writev; writevSync; }, births = [Array, Buffer, Error, Function, Object, Promise]; "node:http", members = { Agent; ClientRequest; IncomingMessage; METHODS; OutgoingMessage; STATUS_CODES; Server; ServerResponse; WebSocket = ref(WebSocket); _connectionListener; createServer; default; get; globalAgent; kConnectionsCheckingInterval = "@@kConnectionsCheckingInterval"; maxHeaderSize = 16384.0; request; setGlobalProxyFromEnv; setMaxIdleHTTPParsers; validateHeaderName; validateHeaderValue; }, births = [Array, Buffer, Error, Function, Object, Promise, URL]; "node:http2", members = { Http2ServerRequest; Http2ServerResponse; connect; constants; createSecureServer; createServer; default; getDefaultSettings; getPackedSettings; getUnpackedSettings; performServerHandshake; sensitiveHeaders = Symbol("sensitiveHeaders"); }, births = [Array, Buffer, Error, Function, Object, Promise, URL]; "node:https", members = { Agent; Server; createServer; default; get; globalAgent; request; }, births = [Array, Buffer, Error, Function, Object, Promise, URL]; "node:inspector", members = { Network; Session; close; console; default; open; url; waitForDebugger; }, births = [Array, Error, Function, Object]; "node:inspector/promises", members = { Session; default; }, births = [Array, Error, Function, Object]; "node:module", members = { Module; SourceMap; _cache; _extensions; _findPath; _initPaths; _load; _nodeModulePaths; _pathCache; _preloadModules; _resolveFilename; _resolveLookupPaths; builtinModules; constants; createRequire; default = ref("node:module.Module"); enableCompileCache; findPackageJSON; findSourceMap; flushCompileCache; getCompileCacheDir; getSourceMapsSupport; globalPaths; isBuiltin; register; registerHooks; runMain; setSourceMapsSupport; stripTypeScriptTypes; syncBuiltinESMExports; }, births = [Array, Error, Function, Object]; "node:net", members = { BlockList; Server; Socket; SocketAddress; Stream = ref("node:net.Socket"); _createServerHandle; _normalizeArgs; connect; createConnection; createServer; default; getDefaultAutoSelectFamily; getDefaultAutoSelectFamilyAttemptTimeout; isIP; isIPv4; isIPv6; setDefaultAutoSelectFamily; setDefaultAutoSelectFamilyAttemptTimeout; }, births = [Array, Buffer, Error, Function, Object, Promise]; "node:os", members = { EOL = "\n"; arch; availableParallelism; constants; cpus; default; devNull = "/dev/null"; endianness; freemem; getPriority; homedir; hostname; loadavg; machine; networkInterfaces; platform; release; setPriority; tmpdir; totalmem; type; uptime; userInfo; version; }, births = [Array, Error, Function, Object]; "node:path", members = { _makeLong; basename; default; delimiter = ":"; dirname; extname; format; isAbsolute; join; matchesGlob; normalize; parse; posix; relative; resolve; sep = "/"; toNamespacedPath = ref("node:path._makeLong"); win32; }, births = [Array, Error, Function, Object]; "node:path/posix", members = { _makeLong; basename; default = undefined; delimiter = ":"; dirname; extname; format; isAbsolute; join; matchesGlob; normalize; parse; posix = ref("node:path.posix"); relative; resolve; sep = "/"; toNamespacedPath = ref("node:path/posix._makeLong"); win32 = ref("node:path.win32"); }, births = [Array, Error, Function, Object]; "node:path/win32", members = { _makeLong; basename; default = undefined; delimiter = ";"; dirname; extname; format; isAbsolute; join; matchesGlob; normalize; parse; posix = ref("node:path.posix"); relative; resolve; sep = "\\"; toNamespacedPath = ref("node:path/win32._makeLong"); win32 = ref("node:path.win32"); }, births = [Array, Error, Function, Object]; "node:perf_hooks", members = { Performance = ref(Performance); PerformanceEntry = ref(PerformanceEntry); PerformanceMark = ref(PerformanceMark); PerformanceMeasure = ref(PerformanceMeasure); PerformanceObserver = ref(PerformanceObserver); PerformanceObserverEntryList = ref(PerformanceObserverEntryList); PerformanceResourceTiming = ref(PerformanceResourceTiming); constants; createHistogram = ref(performance.createHistogram); default = ref(performance.default); eventLoopUtilization = ref(performance.eventLoopUtilization); monitorEventLoopDelay = ref(performance.monitorEventLoopDelay); performance = ref(performance); timerify = ref(performance.timerify); }, births = [Array, Error, EventTarget, Function, Object, Promise]; "node:querystring", members = { decode; default; encode; escape; parse = ref("node:querystring.decode"); stringify = ref("node:querystring.encode"); unescape; unescapeBuffer; }, births = [Array, Error, Function, Object]; "node:readline", members = { clearLine; clearScreenDown; createInterface; cursorTo; default; emitKeypressEvents; moveCursor; promises; }, births = [Array, Error, Function, Object]; "node:repl", members = { REPLServer; REPL_MODE_SLOPPY = Symbol.for("nodejs.repl.mode.sloppy"); REPL_MODE_STRICT = Symbol.for("nodejs.repl.mode.strict"); Recoverable; builtinModules = ref("node:module.builtinModules"); default; start; }, births = [Array, Error, Function, Object]; "node:sea", members = { default; getAsset; getAssetAsBlob; getAssetKeys; getRawAsset; isSea; }, births = [Array, Error, Function, Object]; "node:stream", members = { Duplex; PassThrough; Readable; Stream; Transform; Writable; _isArrayBufferView; _isUint8Array; _uint8ArrayToBuffer; addAbortSignal; compose; default = ref("node:stream.Stream"); duplexPair; finished; getDefaultHighWaterMark; isDestroyed; isDisturbed; isErrored; isReadable; isWritable; pipeline; promises; setDefaultHighWaterMark; }, births = [Array, ArrayBuffer, Buffer, Error, Function, Object, Promise, ReadableStream, WritableStream]; "node:string_decoder", members = { StringDecoder; default; }, births = [Array, Error, Function, Object]; "node:tls", members = { CLIENT_RENEG_LIMIT = 3.0; CLIENT_RENEG_WINDOW = 600.0; DEFAULT_CIPHERS = "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"; DEFAULT_ECDH_CURVE = "auto"; DEFAULT_MAX_VERSION = "TLSv1.3"; DEFAULT_MIN_VERSION = "TLSv1.2"; SecureContext; Server; TLSSocket; checkServerIdentity; connect; convertALPNProtocols; createSecureContext; createServer; default; getCACertificates; getCertificateCompressionAlgorithms; getCiphers; rootCertificates; setDefaultCACertificates; }, births = [Array, Buffer, Error, Function, Object, Promise]; "node:tty", members = { ReadStream; WriteStream; default; isatty; }, births = [Array, Error, Function, Object]; "node:url", members = { URL = ref(URL); URLPattern = ref(URLPattern); URLSearchParams = ref(URLSearchParams); Url; default; domainToASCII; domainToUnicode; fileURLToPath; fileURLToPathBuffer; format; parse; pathToFileURL; resolve; resolveObject; urlToHttpOptions; }, births = [Array, Error, Function, Object]; "node:util", members = { MIMEParams = ref("node:sys.MIMEParams"); MIMEType = ref("node:sys.MIMEType"); TextDecoder = ref(TextDecoder); TextEncoder = ref(TextEncoder); _errnoException = ref("node:sys._errnoException"); _exceptionWithHostPort = ref("node:sys._exceptionWithHostPort"); _extend = ref("node:sys._extend"); aborted = ref("node:sys.aborted"); callbackify = ref("node:sys.callbackify"); convertProcessSignalToExitCode = ref("node:sys.convertProcessSignalToExitCode"); debug = ref("node:sys.debug"); debuglog = ref("node:sys.debug"); default = ref("node:sys.default"); deprecate = ref("node:sys.deprecate"); diff = ref("node:sys.diff"); format = ref("node:sys.format"); formatWithOptions = ref("node:sys.formatWithOptions"); getCallSites = ref("node:sys.getCallSites"); getSystemErrorMap = ref("node:sys.getSystemErrorMap"); getSystemErrorMessage = ref("node:sys.getSystemErrorMessage"); getSystemErrorName = ref("node:sys.getSystemErrorName"); inherits = ref("node:sys.inherits"); inspect = ref("node:sys.inspect"); isArray = ref("node:sys.isArray"); isDeepStrictEqual = ref("node:sys.isDeepStrictEqual"); parseArgs = ref("node:sys.parseArgs"); parseEnv = ref("node:sys.parseEnv"); promisify = ref("node:sys.promisify"); setTraceSigInt = ref("node:sys.setTraceSigInt"); stripVTControlCharacters = ref("node:sys.stripVTControlCharacters"); styleText = ref("node:sys.styleText"); toUSVString = ref("node:sys.toUSVString"); transferableAbortController = ref("node:sys.transferableAbortController"); transferableAbortSignal = ref("node:sys.transferableAbortSignal"); types = ref("node:sys.types"); }, births = [Array, Error, Function, Object, Promise]; "node:util/types", members = { default = ref("node:sys.types"); isAnyArrayBuffer; isArgumentsObject; isArrayBuffer; isArrayBufferView; isAsyncFunction; isBigInt64Array; isBigIntObject; isBigUint64Array; isBooleanObject; isBoxedPrimitive; isCryptoKey; isDataView; isDate; isExternal; isFloat16Array; isFloat32Array; isFloat64Array; isGeneratorFunction; isGeneratorObject; isInt16Array; isInt32Array; isInt8Array; isKeyObject; isMap; isMapIterator; isModuleNamespaceObject; isNativeError; isNumberObject; isPromise; isProxy; isRegExp; isSet; isSetIterator; isSharedArrayBuffer; isStringObject; isSymbolObject; isTypedArray; isUint16Array; isUint32Array; isUint8Array; isUint8ClampedArray; isWeakMap; isWeakSet; }, births = [Array, Error, Function, Object]; "node:v8", members = { DefaultDeserializer; DefaultSerializer; Deserializer; GCProfiler; Serializer; cachedDataVersionTag; default; deserialize; getCppHeapStatistics; getHeapCodeStatistics; getHeapSnapshot; getHeapSpaceStatistics; getHeapStatistics; isStringOneByteRepresentation; promiseHooks; queryObjects; serialize; setFlagsFromString; setHeapSnapshotNearHeapLimit; startCpuProfile; startupSnapshot; stopCoverage; takeCoverage; writeHeapSnapshot; }, births = [Array, ArrayBuffer, BigInt64Array, BigUint64Array, Buffer, DataView, Date, Error, Float32Array, Float64Array, Function, Int16Array, Int32Array, Int8Array, Map, Object, RegExp, Set, Uint16Array, Uint32Array, Uint8Array, Uint8ClampedArray]; "node:vm", members = { Script; compileFunction; constants; createContext; createScript; default; isContext; measureMemory; runInContext; runInNewContext; runInThisContext; }, births = [Array, Error, Function, Object]; "node:wasi", members = { WASI; default; wasiImport = undefined; }, births = [Array, Error, Function, Object]; "node:worker_threads", members = { BroadcastChannel = ref(BroadcastChannel); MessageChannel = ref(MessageChannel); MessagePort = ref(MessagePort); SHARE_ENV = Symbol.for("nodejs.worker_threads.SHARE_ENV"); Worker; default; getEnvironmentData; isInternalThread = false; isMainThread = true; isMarkedAsUntransferable; locks; markAsUncloneable; markAsUntransferable; moveMessagePortToContext; parentPort = null; postMessageToThread; receiveMessageOnPort; resourceLimits; setEnvironmentData; threadId = 0.0; threadName = ""; workerData = null; }, births = [Array, ArrayBuffer, Buffer, Error, Function, Object, Promise, SharedArrayBuffer]; "node:zlib", members = { BrotliCompress; BrotliDecompress; Deflate; DeflateRaw; Gunzip; Gzip; Inflate; InflateRaw; Unzip; ZstdCompress; ZstdDecompress; brotliCompress; brotliCompressSync; brotliDecompress; brotliDecompressSync; codes; constants; crc32; createBrotliCompress; createBrotliDecompress; createDeflate; createDeflateRaw; createGunzip; createGzip; createInflate; createInflateRaw; createUnzip; createZstdCompress; createZstdDecompress; default; deflate; deflateRaw; deflateRawSync; deflateSync; gunzip; gunzipSync; gzip; gzipSync; inflate; inflateRaw; inflateRawSync; inflateSync; unzip; unzipSync; zstdCompress; zstdCompressSync; zstdDecompress; zstdDecompressSync; }, births = [Array, Buffer, Error, Function, Object, Promise]; "node:process", members = { _debugEnd = ref(process._debugEnd); _debugProcess = ref(process._debugProcess); _eval = undefined; _events; _eventsCount = 0.0; _exiting = false; _fatalException = ref(process._fatalException); _getActiveHandles = ref(process._getActiveHandles); _getActiveRequests = ref(process._getActiveRequests); _kill = ref(process._kill); _linkedBinding = ref(process._linkedBinding); _maxListeners = undefined; _preload_modules; _rawDebug = ref(process._rawDebug); _startProfilerIdleNotifier = ref(process._startProfilerIdleNotifier); _stopProfilerIdleNotifier = ref(process._stopProfilerIdleNotifier); _tickCallback = ref(process._tickCallback); abort = ref(process.abort); addUncaughtExceptionCaptureCallback = ref(process.addUncaughtExceptionCaptureCallback); allowedNodeEnvironmentFlags; arch = target(process.arch); argv = runtime(process.argv); argv0 = runtime(process.argv0); availableMemory; binding = ref(process.binding); chdir = ref(process.chdir); config; constrainedMemory; cpuUsage = ref(process.cpuUsage); cwd = ref(process.cwd); debugPort = 9229.0; default = ref(process); dlopen = ref(process.dlopen); domain = null; emitWarning; env = runtime(process.env); execArgv = runtime(process.execArgv); execPath = runtime(process.execPath); execve = ref(process.execve); exit; features; finalization = ref(process.finalization); getActiveResourcesInfo = ref(process.getActiveResourcesInfo); getBuiltinModule = ref(process.getBuiltinModule); getegid; geteuid; getgid; getgroups; getuid; hasUncaughtExceptionCaptureCallback; hrtime = ref(process.hrtime); initgroups; kill = ref(process.kill); loadEnvFile = undefined; memoryUsage = ref(process.memoryUsage); moduleLoadList; nextTick = ref(process.nextTick); openStdin = ref(process.openStdin); permission = undefined; pid = runtime(process.pid); platform = target(process.platform); ppid = runtime(process.ppid); reallyExit = ref(process.reallyExit); ref = ref(process.ref); release; report = ref(process.report); resourceUsage = ref(process.resourceUsage); setSourceMapsEnabled = ref(process.setSourceMapsEnabled); setUncaughtExceptionCaptureCallback; setegid; seteuid; setgid; setgroups; setuid; sourceMapsEnabled = false; stderr = ref(process.stderr); stdin = ref(process.stdin); stdout = ref(process.stdout); threadCpuUsage; title = runtime(process.title); umask; unref = ref(process.unref); uptime = ref(process.uptime); version = "v22.0.0"; versions; }, births = [Array, Buffer, Error, Function, Object, Promise]; } };
    (*obj).class_id = MODULE_NAMESPACE_CLASS_ID;
    let fields = obj
        .cast::<u8>()
        .add(std::mem::size_of::<ObjectHeader>())
        .cast::<u64>();
    let old = crate::arena::pointer_in_old_gen(obj as usize);
    for (i, key) in keys.iter().enumerate() {
        let bits = match &words[i] {
            Some(pair) => pair.get_nanbox_u64(),
            None => value_handles[i].get_nanbox_u64(),
        };
        let slot = fields.add(key.slot as usize);
        // GC_STORE_AUDIT(INIT): a declared slot of the unpublished birth.
        *slot = bits;
        if old {
            crate::gc::runtime_write_barrier_slot(obj as usize, slot as usize, bits);
        }
    }
    crate::value::js_nanbox_pointer(obj as i64)
}

/// [`js_create_namespace_layout`] for a descriptor this runtime did not
/// register: the generic per-key construction, from the descriptor's export
/// names (its string atoms) and accessor bits.
#[cold]
unsafe fn namespace_from_unregistered_layout(
    keys: &[super::layout_desc::PerryLayoutKey],
    values: *const f64,
) -> f64 {
    let mut names = Vec::with_capacity(keys.len());
    let mut lens = Vec::with_capacity(keys.len());
    let mut live = Vec::with_capacity(keys.len());
    let mut ordered = Vec::with_capacity(keys.len());
    debug_assert!(keys.iter().enumerate().all(|(i, k)| k.slot as usize == i));
    for (i, key) in keys.iter().enumerate() {
        // `@@toStringTag` is the generic construction's own business.
        if (*key.atom).kind != crate::codegen_abi::ATOM_KIND_STRING {
            continue;
        }
        let s = (*key.atom).string;
        names.push(crate::string::string_data(s));
        lens.push((*s).byte_len as i32);
        live.push(u8::from(key.attrs & super::key_attrs::ENTRY_ACCESSOR != 0));
        ordered.push(*values.add(i));
    }
    js_create_namespace(
        names.len() as i32,
        names.as_ptr(),
        lens.as_ptr(),
        ordered.as_ptr(),
        live.as_ptr(),
    )
}

// The namespace descriptor's integrity byte is the runtime's flag.
const _: () = assert!(
    crate::codegen_abi::LAYOUT_INTEGRITY_NON_EXTENSIBLE as u16 == crate::gc::OBJ_FLAG_NO_EXTEND
);

#[cfg(test)]
mod sso_tests_1781 {
    use super::*;

    #[test]
    fn get_own_property_names_array_and_string_no_crash() {
        // Regression: getOwnPropertyNames on an array/string read a bogus
        // keys_array off the wrong header and segfaulted. Now returns the
        // index names + "length".
        let arr = crate::array::js_array_alloc(4);
        for v in [10.0, 20.0, 30.0] {
            crate::array::js_array_push_f64(arr, v);
        }
        let arr_val = crate::value::js_nanbox_pointer(arr as i64);
        let names = js_object_get_own_property_names(arr_val);
        let names_ptr =
            (names.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        // 3 indices + "length".
        assert_eq!(crate::array::js_array_length(names_ptr), 4);

        let s = crate::string::js_string_from_bytes(b"ab".as_ptr(), 2);
        let s_val = crate::value::js_nanbox_string(s as i64);
        let s_names = js_object_get_own_property_names(s_val);
        let s_ptr = (s_names.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(s_ptr), 3); // "0","1","length"
    }

    /// #1781: `Object.is` content-compares strings only when both sides pass
    /// `is_string()` (STRING_TAG-only). Two SSO operands match via the
    /// bit-pattern fallback, but a mixed SSO/heap pair with equal content
    /// (e.g. a JSON-parsed value vs a heap literal) did not — `Object.is`
    /// wrongly returned false. Now representation-independent.
    #[test]
    fn object_is_compares_sso_and_mixed_strings() {
        let truthy = |v: f64| crate::value::js_is_truthy(v) != 0;
        let a = JSValue::try_short_string(b"abc").unwrap();
        let b = JSValue::try_short_string(b"abc").unwrap();
        assert!(
            truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(b.bits())
            )),
            "two equal SSO strings"
        );

        let heap = JSValue::string_ptr(crate::string::js_string_from_bytes(b"abc".as_ptr(), 3));
        assert!(
            truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(heap.bits())
            )),
            "mixed SSO/heap, equal content"
        );

        let c = JSValue::try_short_string(b"xyz").unwrap();
        assert!(
            !truthy(js_object_is(
                f64::from_bits(a.bits()),
                f64::from_bits(c.bits())
            )),
            "different content"
        );
    }

    /// #1781: an object with an inline-SSO key must answer
    /// `hasOwnProperty("id")` truthfully. Pre-fix the
    /// `is_string()`-gated keys-array iteration in `own_key_present`
    /// skipped the SSO key silently and the call returned false.
    #[test]
    fn own_key_present_finds_sso_stored_key() {
        unsafe {
            let obj = super::super::alloc::js_object_alloc(0, 4);
            // Build a keys array with a single SSO-tagged key directly
            // (skipping `js_object_set_field_by_name`, which would
            // intern the key to heap and bypass the SSO blind spot
            // we're regression-testing).
            let keys = crate::array::js_array_alloc(4);
            let sso = JSValue::try_short_string(b"id").expect("SSO");
            crate::array::js_array_push_f64(keys, f64::from_bits(sso.bits()));
            super::super::set_object_keys(obj, super::super::ObjectKeys::owned(keys));

            let incoming = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
            assert!(
                own_key_present(obj, incoming),
                "SSO key 'id' should be visible to own_key_present"
            );

            let incoming_other = crate::string::js_string_from_bytes(b"tag".as_ptr(), 3);
            assert!(
                !own_key_present(obj, incoming_other),
                "absent key 'tag' must not match"
            );
        }
    }

    /// #3527: a non-object value reaching `own_key_present` must return
    /// `false`, never SIGBUS. Two shapes seen compiling Express: (a) a
    /// misaligned receiver pointer, and (b) a real-looking object whose
    /// `keys_array` field holds misaligned garbage (the value read from a
    /// native-module namespace sentinel mis-treated as an object). Both are
    /// rejected by the low-3-bits alignment guard — every genuine GC pointer
    /// is `align.max(8)`-aligned.
    #[test]
    fn own_key_present_rejects_misaligned_pointers() {
        unsafe {
            let key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);

            // (a) misaligned receiver — would deref `[obj-8]`/`crate::object::object_keys_array(obj)`
            // on garbage without the guard.
            let misaligned_obj = 0x2800_0203usize as *mut ObjectHeader;
            assert!(
                !own_key_present(misaligned_obj, key),
                "misaligned receiver must return false, not crash"
            );
        }
    }
}
