//! node:http's `OutgoingMessage.prototype`, `ServerResponse.prototype` and
//! `IncomingMessage.prototype` on their static layouts
//! (`runtime_layouts::node_prototypes`: node's own keys, order and
//! attributes), and the `ServerResponse` instance: born on its layout
//! (`runtime_layouts::http_instances`) with node's constructor fields
//! (object model §3.2/§3.6, step 6 slice 3b-ii, http half).
//!
//! Each prototype is born on its layout once per agent, when its constructor
//! export is minted (`native_module::callable_exports::nm_attach_http`),
//! populated in place and sealed with its reserved serial. The methods are
//! one closure each per agent, dispatching the method by name to the native
//! response or request the receiver carries (a heap response carries its
//! payload in `meta.native_state`; a live server's `req`/`res` is a numeric
//! handle). The accessors node defines read the instance fields node's
//! getters read (`writableEnded` is `this.finished`, `headersSent` is
//! `!!this._header`, `closed` is `this._closed`) or ask the native side for
//! what only it holds (the socket, its buffer).
//!
//! Listeners are node's: `on`/`once`/`emit` come from `EventEmitter.prototype`
//! through the chain and keep their data in the instance's `_events`; the
//! native response fires `finish`/`close` through `this.emit`.
use crate::closure::ClosureHeader;
use crate::codegen_abi::LayoutWord;
use crate::object::runtime_layouts as layouts;
use crate::object::ObjectHeader;
use crate::value::JSValue;

/// Which http prototype.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HttpProto {
    OutgoingMessage,
    ServerResponse,
    IncomingMessage,
}

impl HttpProto {
    pub(crate) fn of_export(name: &str) -> Option<Self> {
        Some(match name {
            "OutgoingMessage" => HttpProto::OutgoingMessage,
            "ServerResponse" => HttpProto::ServerResponse,
            "IncomingMessage" => HttpProto::IncomingMessage,
            _ => return None,
        })
    }

    fn layout(self) -> *const crate::object::layout_desc::PerryLayoutDesc {
        match self {
            HttpProto::OutgoingMessage => &raw const layouts::OUTGOING_MESSAGE_PROTOTYPE,
            HttpProto::ServerResponse => &raw const layouts::SERVER_RESPONSE_PROTOTYPE,
            HttpProto::IncomingMessage => &raw const layouts::INCOMING_MESSAGE_PROTOTYPE,
        }
    }

    fn serial(self) -> u64 {
        use crate::closure::shape as s;
        match self {
            HttpProto::OutgoingMessage => s::INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE,
            HttpProto::ServerResponse => s::INTRINSIC_SERIAL_SERVER_RESPONSE_PROTOTYPE,
            HttpProto::IncomingMessage => s::INTRINSIC_SERIAL_INCOMING_MESSAGE_PROTOTYPE,
        }
    }

    fn export_name(self) -> &'static str {
        match self {
            HttpProto::OutgoingMessage => "OutgoingMessage",
            HttpProto::ServerResponse => "ServerResponse",
            HttpProto::IncomingMessage => "IncomingMessage",
        }
    }
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn boxed(obj: *const ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

/// `kind`'s prototype, built now (by minting its constructor export) if this
/// agent has none yet.
fn ensure_http_prototype(kind: HttpProto) -> f64 {
    if let Some(proto) = layouts::node_prototype_of_serial(kind.serial()) {
        return boxed(proto);
    }
    let _ = crate::object::bound_native_callable_export_value("http", kind.export_name());
    layouts::node_prototype_of_serial(kind.serial()).map_or_else(undefined, |p| boxed(p))
}

/// The parent prototype `kind`'s layout names exists (its identity word
/// names it) before `kind` is born on that layout.
fn ensure_parent(kind: HttpProto) {
    use crate::closure::shape as s;
    match kind {
        HttpProto::OutgoingMessage => {
            if layouts::node_prototype_of_serial(s::INTRINSIC_SERIAL_STREAM_PROTOTYPE).is_none() {
                let _ = crate::node_stream::stream_prototype_value("Stream");
            }
        }
        HttpProto::ServerResponse => {
            let _ = ensure_http_prototype(HttpProto::OutgoingMessage);
        }
        HttpProto::IncomingMessage => {
            if layouts::node_prototype_of_serial(s::INTRINSIC_SERIAL_READABLE_PROTOTYPE).is_none() {
                let _ = crate::node_stream::stream_prototype_value("Readable");
            }
        }
    }
}

/// The http module's attach hook for `name`'s constructor: its prototype is
/// built on its layout and becomes the constructor's `prototype`.
pub(crate) fn attach_http_prototype(constructor_value: f64, name: &str) -> f64 {
    let Some(kind) = HttpProto::of_export(name) else {
        return constructor_value;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor_value);
    let ctor = JSValue::from_bits(constructor_value.to_bits());
    if !ctor.is_pointer()
        || !crate::closure::is_closure_ptr(
            (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        )
    {
        return constructor_value;
    }
    let proto = scope.root_nanbox_f64(build_http_prototype(constructor.get_nanbox_f64(), kind));
    let closure_addr =
        (constructor.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    crate::closure::closure_set_dynamic_prop(closure_addr, "prototype", proto.get_nanbox_f64());
    crate::object::set_builtin_property_attrs(
        closure_addr,
        "prototype".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
    constructor.get_nanbox_f64()
}

/// One closure of `info` whose captures name the method `name`.
fn named_closure(
    scope: &crate::gc::RuntimeHandleScope,
    info: *const crate::closure::JsFunctionInfo,
    name: &'static str,
    length: u32,
) -> f64 {
    let closure = crate::closure::js_closure_alloc(info, 2);
    crate::closure::closure_set_capture_word(closure, 0, name.as_ptr() as i64);
    crate::closure::closure_set_capture_word(closure, 1, name.len() as i64);
    let closure = scope.root_raw_mut_ptr(closure);
    closure.with_mut_ptr(|c| crate::object::set_bound_native_closure_name(c, name));
    closure
        .with_mut_ptr(|c: *mut u8| crate::object::set_builtin_closure_length(c as usize, length));
    closure.with_mut_ptr(|c: *mut u8| crate::value::js_nanbox_pointer(c as i64))
}

/// Select immutable method metadata from the already installed HTTP provider
/// while building the prototype. No call site or receiver state is retained.
fn method_info(name: &str) -> *const crate::closure::JsFunctionInfo {
    let ptr = crate::value::JS_NATIVE_HTTP_DISPATCH.load(std::sync::atomic::Ordering::Acquire);
    if !ptr.is_null() && matches!(name, "setHeader" | "writeHead" | "write" | "end") {
        let dispatch: crate::value::JsNativeHttpDispatchFn = unsafe { std::mem::transmute(ptr) };
        let module = b"#http-prototype-method-info";
        let info = unsafe {
            dispatch(
                module.as_ptr(),
                module.len(),
                name.as_ptr(),
                name.len(),
                std::ptr::null(),
                0,
            )
        }
        .to_bits();
        if info != 0 && info != crate::value::TAG_UNDEFINED {
            return info as *const crate::closure::JsFunctionInfo;
        }
    }
    crate::fn_info!(#[realm(realm_decl! { "node:http.IncomingMessage.prototype._addHeaderLine", capture = property_key, constructible = true; "node:http.IncomingMessage.prototype._addHeaderLineDistinct", capture = property_key, constructible = true; "node:http.OutgoingMessage.prototype._send", capture = property_key, constructible = true; "node:http.OutgoingMessage.prototype._writeRaw", capture = property_key, constructible = true; "node:http.IncomingMessage.prototype._addHeaderLines", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_incomingmessage_prototype_addheaderlines_metadata); "node:http.IncomingMessage.prototype._destroy", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_incomingmessage_prototype_destroy_metadata); "node:http.IncomingMessage.prototype._dump", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_incomingmessage_prototype_dump_metadata); "node:http.IncomingMessage.prototype._dumpAndCloseReadable", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_incomingmessage_prototype_dumpandclosereadable_metadata); "node:http.IncomingMessage.prototype._read", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_incomingmessage_prototype_read_metadata); "node:http.IncomingMessage.prototype.setTimeout", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_incomingmessage_prototype_settimeout_metadata); "node:http.OutgoingMessage.prototype._finish", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_finish_metadata); "node:http.OutgoingMessage.prototype._flush", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_flush_metadata); "node:http.OutgoingMessage.prototype._flushOutput", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_flushoutput_metadata); "node:http.OutgoingMessage.prototype._implicitHeader", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_implicitheader_metadata); "node:http.OutgoingMessage.prototype._isLenientHeaderValidation", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_islenientheadervalidation_metadata); "node:http.OutgoingMessage.prototype._renderHeaders", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_renderheaders_metadata); "node:http.OutgoingMessage.prototype._storeHeader", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_outgoingmessage_prototype_storeheader_metadata); "node:http.OutgoingMessage.prototype.addTrailers", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_addtrailers_metadata); "node:http.OutgoingMessage.prototype.appendHeader", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_outgoingmessage_prototype_appendheader_metadata); "node:http.OutgoingMessage.prototype.cork", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_cork_metadata); "node:http.OutgoingMessage.prototype.destroy", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_destroy_metadata); "node:http.OutgoingMessage.prototype.flushHeaders", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_flushheaders_metadata); "node:http.OutgoingMessage.prototype.getHeader", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_getheader_metadata); "node:http.OutgoingMessage.prototype.getHeaderNames", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_getheadernames_metadata); "node:http.OutgoingMessage.prototype.getHeaders", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_getheaders_metadata); "node:http.OutgoingMessage.prototype.getRawHeaderNames", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_getrawheadernames_metadata); "node:http.OutgoingMessage.prototype.hasHeader", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_hasheader_metadata); "node:http.OutgoingMessage.prototype.pipe", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_pipe_metadata); "node:http.OutgoingMessage.prototype.removeHeader", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_removeheader_metadata); "node:http.OutgoingMessage.prototype.setHeaders", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_setheaders_metadata); "node:http.OutgoingMessage.prototype.setTimeout", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_outgoingmessage_prototype_settimeout_metadata); "node:http.OutgoingMessage.prototype.uncork", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_uncork_metadata); "node:http.ServerResponse.prototype._finish", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_serverresponse_prototype_finish_metadata); "node:http.ServerResponse.prototype._implicitHeader", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_serverresponse_prototype_implicitheader_metadata); "node:http.ServerResponse.prototype.assignSocket", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_assignsocket_metadata); "node:http.ServerResponse.prototype.detachSocket", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_detachsocket_metadata); "node:http.ServerResponse.prototype.writeContinue", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_writecontinue_metadata); "node:http.ServerResponse.prototype.writeEarlyHints", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_serverresponse_prototype_writeearlyhints_metadata); "node:http.ServerResponse.prototype.writeInformation", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_serverresponse_prototype_writeinformation_metadata); "node:http.ServerResponse.prototype.writeProcessing", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_writeprocessing_metadata); "node:http.IncomingMessage.prototype[\"@runtime:kAttachAbortSignal\"]", capture = public_name, constructible = true, spec = {name = "kAttachAbortSignal", length = 0}, emit = metadata(realm_http_symbol_attach_public); "node:http.IncomingMessage.prototype[\"@runtime:kDetachAbortSignal\"]", capture = public_name, constructible = true, spec = {name = "kDetachAbortSignal", length = 0}, emit = metadata(realm_http_symbol_detach_public); "node:http.OutgoingMessage.prototype[\"@runtime:nodejs.rejection\"]", capture = public_name, constructible = true, spec = {name = "destroy", length = 0}, emit = metadata(realm_http_symbol_rejection_public); })] js_node_http_method_fallback, 3; with_declared(3))
}

/// Build `kind`'s prototype for `constructor`: born on its layout, every slot
/// filled, sealed.
#[rustfmt::skip] fn build_http_prototype(constructor: f64, kind: HttpProto) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    ensure_parent(kind);
    // Building the parent mints exports, which may have built this one.
    if let Some(proto) = layouts::node_prototype_of_serial(kind.serial()) {
        return boxed(proto);
    }
    // SAFETY: a node prototype descriptor of this runtime.
    let layout = unsafe { layouts::NodePrototype::of(kind.layout()) };
    let proto = scope.root_raw_mut_ptr(unsafe { layout.birth() });
    let p = || proto.get_raw_mut_ptr::<ObjectHeader>();
    // SAFETY (every fill below): the fresh prototype born on `layout`, not yet
    // reachable by program code.
    let fill = |name: &str, value: f64| unsafe { layout.data(p(), name.as_bytes(), value) };
    fill("constructor", constructor.get_nanbox_f64());
    let method = |name: &'static str, length: u32| {
        let value = named_closure(&scope, method_info(name), name, length);
        fill(name, value);
    };
    let accessor = |name: &'static str, setter: bool| {
        let get = named_closure(&scope, crate::fn_info!(#[realm(realm_decl! { "node:http.IncomingMessage.prototype.connection.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_connection_get_name_metadata); "node:http.IncomingMessage.prototype.headers.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headers_get_name_metadata); "node:http.IncomingMessage.prototype.headersDistinct.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headersdistinct_get_name_metadata); "node:http.IncomingMessage.prototype.signal.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_signal_get_name_metadata); "node:http.IncomingMessage.prototype.trailers.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailers_get_name_metadata); "node:http.IncomingMessage.prototype.trailersDistinct.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailersdistinct_get_name_metadata); "node:http.OutgoingMessage.prototype.closed.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_closed_get_name_metadata); "node:http.OutgoingMessage.prototype.connection.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_connection_get_name_metadata); "node:http.OutgoingMessage.prototype.errored.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_errored_get_name_metadata); "node:http.OutgoingMessage.prototype.headersSent.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_headerssent_get_name_metadata); "node:http.OutgoingMessage.prototype.socket.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_socket_get_name_metadata); "node:http.OutgoingMessage.prototype.writableCorked.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablecorked_get_name_metadata); "node:http.OutgoingMessage.prototype.writableEnded.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writableended_get_name_metadata); "node:http.OutgoingMessage.prototype.writableFinished.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablefinished_get_name_metadata); "node:http.OutgoingMessage.prototype.writableHighWaterMark.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablehighwatermark_get_name_metadata); "node:http.OutgoingMessage.prototype.writableLength.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablelength_get_name_metadata); "node:http.OutgoingMessage.prototype.writableNeedDrain.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writableneeddrain_get_name_metadata); "node:http.OutgoingMessage.prototype.writableObjectMode.<get>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writableobjectmode_get_name_metadata); })] http_getter_thunk, 0), name, 0);
        let get = scope.root_nanbox_f64(get);
        let set = if setter {
            named_closure(&scope, crate::fn_info!(#[realm(realm_decl! { "node:http.IncomingMessage.prototype.connection.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_connection_get_name_metadata); "node:http.IncomingMessage.prototype.headers.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headers_get_name_metadata); "node:http.IncomingMessage.prototype.headersDistinct.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headersdistinct_get_name_metadata); "node:http.IncomingMessage.prototype.trailers.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailers_get_name_metadata); "node:http.IncomingMessage.prototype.trailersDistinct.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailersdistinct_get_name_metadata); "node:http.OutgoingMessage.prototype.connection.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_connection_get_name_metadata); "node:http.OutgoingMessage.prototype.socket.<set>", capture = property_key, constructible = true, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_socket_get_name_metadata); })] http_setter_thunk, 1), name, 1).to_bits()
        } else {
            0
        };
        let set = scope.root_nanbox_f64(f64::from_bits(set));
        // SAFETY: as above; the key is an accessor of the layout.
        unsafe {
            layout.accessor(
                p(),
                name.as_bytes(),
                get.get_nanbox_u64(),
                if setter { set.get_nanbox_u64() } else { 0 },
            )
        };
    };
    let symbol = |atom: *const crate::object::layout_desc::StaticAtom, name: &'static str, length: u32| {
        let value = named_closure(&scope, method_info(name), name, length);
        // SAFETY: as above; `atom` is a key of the layout.
        unsafe { layout.symbol_data(p(), atom, value) };
    };
    match kind {
        HttpProto::OutgoingMessage => {
            method("_isLenientHeaderValidation", realm_node_http_outgoingmessage_prototype_islenientheadervalidation_metadata!(length));
            for name in [
                realm_node_http_outgoingmessage_prototype_errored_get_name_metadata!(name),
                realm_node_http_outgoingmessage_prototype_closed_get_name_metadata!(name),
                realm_node_http_outgoingmessage_prototype_writablefinished_get_name_metadata!(name),
                realm_node_http_outgoingmessage_prototype_writableobjectmode_get_name_metadata!(name),
                realm_node_http_outgoingmessage_prototype_writablelength_get_name_metadata!(name),
                realm_node_http_outgoingmessage_prototype_writablehighwatermark_get_name_metadata!(name),
                realm_node_http_outgoingmessage_prototype_writablecorked_get_name_metadata!(name),
            ] {
                accessor(name, false);
            }
            accessor(realm_node_http_outgoingmessage_prototype_connection_get_name_metadata!(name), true);
            accessor(realm_node_http_outgoingmessage_prototype_socket_get_name_metadata!(name), true);
            for (name, length) in [
                ("_renderHeaders", realm_node_http_outgoingmessage_prototype_renderheaders_metadata!(length)),
                ("cork", realm_node_http_outgoingmessage_prototype_cork_metadata!(length)),
                ("uncork", realm_node_http_outgoingmessage_prototype_uncork_metadata!(length)),
                ("setTimeout", realm_node_http_outgoingmessage_prototype_settimeout_metadata!(length)),
                ("destroy", realm_node_http_outgoingmessage_prototype_destroy_metadata!(length)),
                ("_send", 3),
                ("_writeRaw", 3),
                ("_storeHeader", realm_node_http_outgoingmessage_prototype_storeheader_metadata!(length)),
                ("setHeader", realm_http_set_header_public!(length)),
                ("setHeaders", realm_node_http_outgoingmessage_prototype_setheaders_metadata!(length)),
                ("appendHeader", realm_node_http_outgoingmessage_prototype_appendheader_metadata!(length)),
                ("getHeader", realm_node_http_outgoingmessage_prototype_getheader_metadata!(length)),
                ("getHeaderNames", realm_node_http_outgoingmessage_prototype_getheadernames_metadata!(length)),
                ("getRawHeaderNames", realm_node_http_outgoingmessage_prototype_getrawheadernames_metadata!(length)),
                ("getHeaders", realm_node_http_outgoingmessage_prototype_getheaders_metadata!(length)),
                ("hasHeader", realm_node_http_outgoingmessage_prototype_hasheader_metadata!(length)),
                ("removeHeader", realm_node_http_outgoingmessage_prototype_removeheader_metadata!(length)),
                ("_implicitHeader", realm_node_http_outgoingmessage_prototype_implicitheader_metadata!(length)),
            ] {
                method(name, length);
            }
            accessor(realm_node_http_outgoingmessage_prototype_headerssent_get_name_metadata!(name), false);
            accessor(realm_node_http_outgoingmessage_prototype_writableended_get_name_metadata!(name), false);
            accessor(realm_node_http_outgoingmessage_prototype_writableneeddrain_get_name_metadata!(name), false);
            for (name, length) in [
                ("write", 3),
                ("addTrailers", realm_node_http_outgoingmessage_prototype_addtrailers_metadata!(length)),
                ("end", 3),
                ("_finish", realm_node_http_outgoingmessage_prototype_finish_metadata!(length)),
                ("_flush", realm_node_http_outgoingmessage_prototype_flush_metadata!(length)),
                ("_flushOutput", realm_node_http_outgoingmessage_prototype_flushoutput_metadata!(length)),
                ("flushHeaders", realm_node_http_outgoingmessage_prototype_flushheaders_metadata!(length)),
                ("pipe", realm_node_http_outgoingmessage_prototype_pipe_metadata!(length)),
            ] {
                method(name, length);
            }
            // node: `this.destroy(error)`.
            symbol(&raw const layouts::NP_SYMBOL_NODEJS_REJECTION, realm_http_symbol_rejection_public!(name), realm_http_symbol_rejection_public!(length));
        }
        HttpProto::ServerResponse => {
            method("_finish", realm_node_http_serverresponse_prototype_finish_metadata!(length));
            fill("statusCode", 200.0);
            fill("statusMessage", undefined());
            for (name, length) in [
                ("assignSocket", realm_node_http_serverresponse_prototype_assignsocket_metadata!(length)),
                ("detachSocket", realm_node_http_serverresponse_prototype_detachsocket_metadata!(length)),
                ("writeInformation", realm_node_http_serverresponse_prototype_writeinformation_metadata!(length)),
                ("writeContinue", realm_node_http_serverresponse_prototype_writecontinue_metadata!(length)),
                ("writeProcessing", realm_node_http_serverresponse_prototype_writeprocessing_metadata!(length)),
                ("writeEarlyHints", realm_node_http_serverresponse_prototype_writeearlyhints_metadata!(length)),
                ("_implicitHeader", realm_node_http_serverresponse_prototype_implicitheader_metadata!(length)),
                ("writeHead", 3),
            ] {
                method(name, length);
            }
        }
        HttpProto::IncomingMessage => {
            for name in [
                realm_node_http_incomingmessage_prototype_connection_get_name_metadata!(name),
                realm_node_http_incomingmessage_prototype_headers_get_name_metadata!(name),
                realm_node_http_incomingmessage_prototype_headersdistinct_get_name_metadata!(name),
                realm_node_http_incomingmessage_prototype_trailers_get_name_metadata!(name),
                realm_node_http_incomingmessage_prototype_trailersdistinct_get_name_metadata!(name),
            ] {
                accessor(name, true);
            }
            accessor(realm_node_http_incomingmessage_prototype_signal_get_name_metadata!(name), false);
            for (name, length) in [
                ("setTimeout", realm_node_http_incomingmessage_prototype_settimeout_metadata!(length)),
                ("_read", realm_node_http_incomingmessage_prototype_read_metadata!(length)),
                ("_destroy", realm_node_http_incomingmessage_prototype_destroy_metadata!(length)),
                ("_addHeaderLines", realm_node_http_incomingmessage_prototype_addheaderlines_metadata!(length)),
                ("_addHeaderLine", 3),
                ("_addHeaderLineDistinct", 3),
                ("_dumpAndCloseReadable", realm_node_http_incomingmessage_prototype_dumpandclosereadable_metadata!(length)),
                ("_dump", realm_node_http_incomingmessage_prototype_dump_metadata!(length)),
            ] {
                method(name, length);
            }
            symbol(
                &raw const layouts::NP_SYMBOL_K_ATTACH_ABORT_SIGNAL,
                realm_http_symbol_attach_public!(name), realm_http_symbol_attach_public!(length),
            );
            symbol(
                &raw const layouts::NP_SYMBOL_K_DETACH_ABORT_SIGNAL,
                realm_http_symbol_detach_public!(name), realm_http_symbol_detach_public!(length),
            );
        }
    }
    if kind == HttpProto::OutgoingMessage {
        // node's module-level `nop`, which every message's `_onPendingData`
        // names: one per agent, kept on the prototype's meta record (a traced
        // word no program reaches), so every birth stores the same function.
        let nop = scope.root_nanbox_f64(named_closure(
            &scope,
            crate::fn_info!(http_nop_thunk, 0),
            "nop",
            0,
        ));
        proto.with_mut_ptr(|proto: *mut ObjectHeader| unsafe {
            let meta = crate::object::object_meta_ensure(
                crate::meta_obj_runtime_born!(proto),
                crate::object::MetaReason::HttpPrototypes,
            );
            if !meta.is_null() {
                (*meta).native_state = nop.get_nanbox_u64();
                crate::gc::runtime_write_barrier_slot(
                    meta as usize,
                    &(*meta).native_state as *const _ as usize,
                    nop.get_nanbox_u64(),
                );
            }
        });
    }
    // SAFETY: populated; program code has not seen it.
    unsafe { layout.seal(p(), kind.serial()) };
    proto.with_mut_ptr::<ObjectHeader, _>(|proto| boxed(proto))
}

/// The `nop` every message's `_onPendingData` names.
fn pending_data_nop() -> f64 {
    let Some(proto) = layouts::node_prototype_of_serial(
        crate::closure::shape::INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE,
    ) else {
        return undefined();
    };
    // SAFETY: the agent's live, sealed OutgoingMessage.prototype.
    let meta = unsafe { crate::meta_read!(crate::meta_obj_runtime_born!(proto as *mut _)) };
    if meta.is_null() {
        return undefined();
    }
    f64::from_bits(unsafe { (*meta).native_state })
}

extern "C" fn http_nop_thunk(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    undefined()
}

/// The receiver a prototype method dispatches on: a live server's numeric
/// handle, or a heap object carrying a native response or request payload.
fn native_receiver(receiver: f64) -> Option<f64> {
    let v = JSValue::from_bits(receiver.to_bits());
    if v.is_handle() {
        return Some(receiver);
    }
    let obj = crate::object::field_get_set::runtime_read_site::object_receiver(receiver)?;
    // A1_UNPROVEN: record-first: native payload receiver (kind-A/B question for S0f).
    // SAFETY: a live ordinary object.
    let meta = unsafe { crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _)) };
    if meta.is_null() {
        return None;
    }
    // SAFETY: the meta record of a live object.
    crate::native_payload::payload_cell_of_word(unsafe { (*meta).native_state }).map(|_| receiver)
}

/// An ordinary receiver layout carrying the response payload's native type
/// word. Own-key growth may change the ordinary layout; its current word and
/// the payload's physical type still prove the carrier. The binding separately
/// checks the exact payload vtable before dereferencing Rust response data.
/// Method lookup already chose the body, and the operation keeps its normal
/// observable reads. No prototype chain or key list is searched here.
#[no_mangle]
pub extern "C" fn js_node_http_response_layout_receiver(receiver: f64) -> i32 {
    let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(receiver)
    else {
        return 0;
    };
    unsafe {
        let word = (*obj).parent_class_id;
        if !crate::object::shapes::shape_object_kind_for_word(LayoutWord(word))
            .is_some_and(|kind| kind.is_ordinary_layout())
        {
            return 0;
        }
        // A1_UNPROVEN: record-first: receiver layout (kind-A/B question for S0f).
        let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
        if meta.is_null() {
            return 0;
        }
        let Some(cell) = crate::native_payload::payload_cell_of_word((*meta).native_state) else {
            return 0;
        };
        ((*cell).type_id as u32 == crate::codegen_abi::native_class_ids::HTTP_SERVER_RESPONSE)
            as i32
    }
}

unsafe fn captured_name(closure: *const ClosureHeader) -> (*const u8, usize) {
    (
        crate::closure::js_closure_get_capture_ptr(closure, 0) as *const u8,
        crate::closure::js_closure_get_capture_ptr(closure, 1) as usize,
    )
}

/// Complete fallback for HTTP methods whose receiver is outside the direct
/// response layout: dispatch by name to the native object `this` carries. Chainable methods return the receiver, as
/// node returns `this`.
#[no_mangle]
pub extern "C" fn js_node_http_method_fallback(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> f64 {
    let (name_ptr, name_len) = unsafe { captured_name(closure) };
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(this.as_f64());
    let Some(target) = native_receiver(receiver.get_nanbox_f64()) else {
        let name = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len))
        };
        let msg = format!("The \"{name}\" method was called on an incompatible receiver");
        crate::object::object_ops::throw_object_type_error(msg.as_bytes());
    };
    let Some(dispatch) = crate::object::class_handles::handle_method_dispatch() else {
        return undefined();
    };
    let args = [arg0, arg1, arg2];
    let mut len = args.len();
    while len > 0 && args[len - 1].to_bits() == crate::value::TAG_UNDEFINED {
        len -= 1;
    }
    let result = unsafe {
        dispatch(
            (target.to_bits() & crate::value::POINTER_MASK) as i64,
            name_ptr,
            name_len,
            args.as_ptr(),
            len,
        )
    };
    let target_bits = receiver.get_nanbox_u64() & crate::value::POINTER_MASK;
    if JSValue::from_bits(result.to_bits()).is_reference()
        && result.to_bits() & crate::value::POINTER_MASK == target_bits
    {
        receiver.get_nanbox_f64()
    } else {
        result
    }
}

fn bool_value(b: bool) -> f64 {
    f64::from_bits(if b {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// Body of every http prototype getter. A heap message answers from the
/// fields node's getter reads; the socket and the native buffer come from
/// the native side, as node's getters read them from `kSocket`.
extern "C" fn http_getter_thunk(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let (name_ptr, name_len) = unsafe { captured_name(closure) };
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let receiver = this.as_f64();
    let heap = !JSValue::from_bits(receiver.to_bits()).is_handle()
        && crate::object::field_get_set::runtime_read_site::object_receiver(receiver).is_some();
    if heap {
        let field = |key: &'static [u8]| {
            crate::runtime_state_key::NamedStateKey {
                bytes: key,
                hash: crate::runtime_state_key::const_key_hash(key),
                memo: None,
            }
            .read_value(receiver)
        };
        match name {
            b"writableEnded" => return field(b"finished"),
            b"headersSent" => {
                return bool_value(crate::value::js_is_truthy(field(b"_header")) != 0)
            }
            b"closed" => return field(b"_closed"),
            b"writableObjectMode" => return bool_value(false),
            b"errored" => return f64::from_bits(crate::value::TAG_NULL),
            b"writableNeedDrain" => {
                if crate::value::js_is_truthy(field(b"destroyed")) != 0
                    || crate::value::js_is_truthy(field(b"finished")) != 0
                {
                    return bool_value(false);
                }
            }
            _ => {}
        }
        if native_receiver(receiver).is_none() {
            return undefined();
        }
    }
    match crate::object::class_handles::handle_property_dispatch() {
        Some(dispatch) => unsafe {
            dispatch(
                (receiver.to_bits() & crate::value::POINTER_MASK) as i64,
                name_ptr,
                name_len,
            )
        },
        None => undefined(),
    }
}

/// Body of every http prototype setter: the native side keeps the socket and
/// the request's header views.
extern "C" fn http_setter_thunk(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let (name_ptr, name_len) = unsafe { captured_name(closure) };
    let receiver = this.as_f64();
    if native_receiver(receiver).is_some() {
        if let Some(dispatch) = crate::object::class_handles::handle_property_set_dispatch() {
            unsafe {
                dispatch(
                    (receiver.to_bits() & crate::value::POINTER_MASK) as i64,
                    name_ptr,
                    name_len,
                    value,
                )
            };
        }
    }
    undefined()
}

/// node's `ServerResponse` constructor on `this`: `OutgoingMessage.call(this)`
/// (the emitter's init, then its 24 fields in order), then the response's
/// own. On an object born on the response layout each store fills its slot
/// (`instance_birth::layout_birth_store`); on any other object (a subclass
/// instance, `ServerResponse.call(this, req)`) each is the ordinary `[[Set]]`
/// node's constructor performs.
///
/// # Safety
/// FFI entry: `this` and `req` are live JS values.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_response_init(this: f64, req: f64) {
    message_init(this, Some(req));
}

/// node's `OutgoingMessage` constructor on `this` (`OutgoingMessage.call(this)`
/// or `super()` of a class extending it, through the http extension's
/// dispatcher): the emitter's init, then its fields in order, each the
/// ordinary `[[Set]]`.
///
/// # Safety
/// FFI entry: `this` is a live JS value.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_outgoing_message_init(this: f64) {
    message_init(this, None);
}

/// Node's IncomingMessage constructor on its explicit receiver. Readable owns
/// stream-state initialization; the remaining stores follow Node's constructor
/// order and use ordinary [[Get]]/[[Set]] so inherited accessors participate.
///
/// # Safety
/// `this` and `socket` are live JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_incoming_message_init(this: f64, socket: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let socket = scope.root_nanbox_f64(socket);
    let read = |value, bytes: &'static [u8]| {
        crate::runtime_state_key::NamedStateKey {
            bytes,
            hash: crate::runtime_state_key::const_key_hash(bytes),
            memo: None,
        }
        .read_value(value)
    };
    let put = |name: &'static [u8], value| {
        crate::runtime_state_key::js_runtime_state_key_set(
            this.get_nanbox_f64(),
            name.as_ptr(),
            name.len(),
            value,
        );
    };
    let options = if crate::value::js_is_truthy(socket.get_nanbox_f64()) != 0 {
        let high_water_mark =
            scope.root_nanbox_f64(read(socket.get_nanbox_f64(), b"readableHighWaterMark"));
        let options = scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 1)));
        crate::runtime_state_key::js_runtime_state_key_set(
            options.get_nanbox_f64(),
            b"highWaterMark".as_ptr(),
            13,
            high_water_mark.get_nanbox_f64(),
        );
        options.get_nanbox_f64()
    } else {
        undefined()
    };
    let options = scope.root_nanbox_f64(options);
    // Reuse the Readable explicit-this path, including its instanceof check:
    // an unrelated receiver gets a fresh discarded Readable, as in Node.
    crate::object::native_module_registry::js_nm_install_stream();
    let readable = scope.root_nanbox_f64(crate::object::bound_native_callable_export_value(
        "stream", "Readable",
    ));
    let args = [options.get_nanbox_f64()];
    let _ = crate::object::native_this_alias::maybe_run_stream_subclass_init_via_this(
        readable.get_nanbox_f64(),
        this.get_nanbox_f64(),
        args.as_ptr(),
        args.len(),
    );
    let state = scope.root_nanbox_f64(read(this.get_nanbox_f64(), b"_readableState"));
    let state_value = JSValue::from_bits(state.get_nanbox_u64());
    if state_value.is_undefined() || state_value.is_null() {
        crate::object::object_ops::throw_object_type_error(if state_value.is_null() {
            b"Cannot set properties of null (setting 'readingMore')".as_slice()
        } else {
            b"Cannot set properties of undefined (setting 'readingMore')".as_slice()
        });
    }
    crate::runtime_state_key::js_runtime_state_key_set(
        state.get_nanbox_f64(),
        b"readingMore".as_ptr(),
        11,
        bool_value(true),
    );
    let null = f64::from_bits(crate::value::TAG_NULL);
    put(b"socket", socket.get_nanbox_f64());
    for name in [
        b"httpVersionMajor".as_slice(),
        b"httpVersionMinor",
        b"httpVersion",
    ] {
        put(name, null);
    }
    put(b"complete", bool_value(false));
    put(
        b"rawHeaders",
        crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64),
    );
    put(
        b"rawTrailers",
        crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64),
    );
    put(b"joinDuplicateHeaders", bool_value(false));
    put(b"aborted", bool_value(false));
    put(b"upgrade", null);
    put(
        b"url",
        crate::value::nanbox_string_key(crate::string::js_string_from_bytes(b"".as_ptr(), 0)),
    );
    put(b"method", null);
    put(b"statusCode", null);
    put(b"statusMessage", null);
    put(b"client", socket.get_nanbox_f64());
    put(b"_consuming", bool_value(false));
    put(b"_dumped", bool_value(false));
    use layouts as p;
    for (atom, value) in [
        (&p::NP_SYMBOL_K_HEADERS, null),
        (&p::NP_SYMBOL_K_HEADERS_COUNT, 0.0),
        (&p::NP_SYMBOL_K_TRAILERS, null),
        (&p::NP_SYMBOL_K_TRAILERS_COUNT, 0.0),
        (&p::NP_SYMBOL_K_ABORT_CONTROLLER, null),
        (&p::NP_SYMBOL_K_ABORT_SIGNAL_SOCKET, null),
        (&p::NP_SYMBOL_K_ABORT_SIGNAL_LISTENER, null),
        (&p::NP_SYMBOL_K_ABORT_SIGNAL_DETACHED, bool_value(false)),
    ] {
        let symbol = p::runtime_symbol_value(atom);
        crate::symbol::js_object_set_symbol_property(this.get_nanbox_f64(), symbol, value);
    }
}

/// [`js_node_http_server_response_init`] with `req`, and its
/// `OutgoingMessage` part alone without. Inlined into each entry, so the
/// response's stays specialized to its request.
#[inline(always)]
unsafe fn message_init(this: f64, req: Option<f64>) {
    use crate::object::runtime_layouts::{emitter_slot, response_slot as s};
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let Some(receiver) =
        crate::object::field_get_set::runtime_read_site::object_receiver(this.get_nanbox_f64())
    else {
        return;
    };
    // Born on the response layout, or on its keys under a `new F` whose
    // chain reaches `ServerResponse.prototype` through plain levels: one
    // chain check here, then each store fills its slot while the object
    // stays on those keys.
    let plain_chain = crate::node_stream::instance_birth::on_layout_with_plain_chain(
        receiver,
        &raw const layouts::SERVER_RESPONSE,
    );
    let on_layout = || {
        plain_chain
            && crate::object::field_get_set::runtime_read_site::object_receiver(
                this.get_nanbox_f64(),
            )
            .is_some_and(|obj| {
                crate::node_stream::instance_birth::instance_layout(obj)
                    == Some(&raw const layouts::SERVER_RESPONSE)
            })
    };
    let put = |slot: u32, name: &'static [u8], value: f64| {
        let obj =
            crate::object::field_get_set::runtime_read_site::object_receiver(this.get_nanbox_f64())
                .expect("a live response");
        let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
        if on_layout() && *fields.add(slot as usize) == crate::value::TAG_HOLE {
            crate::node_stream::instance_birth::birth_store(obj, slot, value);
        } else {
            crate::runtime_state_key::js_runtime_state_key_set(
                this.get_nanbox_f64(),
                name.as_ptr(),
                name.len(),
                value,
            );
        }
    };
    let f = bool_value(false);
    let t = bool_value(true);
    let null = f64::from_bits(crate::value::TAG_NULL);
    // EventEmitter.init.
    if on_layout() {
        let events = crate::node_stream::new_events_object();
        put(emitter_slot::EVENTS, b"_events", events);
        put(emitter_slot::EVENTS_COUNT, b"_eventsCount", 0.0);
        put(emitter_slot::MAX_LISTENERS, b"_maxListeners", undefined());
    } else {
        crate::node_stream::init_event_emitter_state(this.get_nanbox_f64());
    }
    // OutgoingMessage's fields, in its constructor's order.
    let output = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::array::js_array_alloc(0) as i64,
    ));
    put(s::OUTPUT_DATA, b"outputData", output.get_nanbox_f64());
    put(s::OUTPUT_SIZE, b"outputSize", 0.0);
    put(s::WRITABLE, b"writable", t);
    put(s::DESTROYED, b"destroyed", f);
    put(s::LAST, b"_last", f);
    put(s::CHUNKED_ENCODING, b"chunkedEncoding", f);
    put(s::SHOULD_KEEP_ALIVE, b"shouldKeepAlive", t);
    put(
        s::MAX_REQUESTS_ON_CONNECTION_REACHED,
        b"maxRequestsOnConnectionReached",
        f,
    );
    put(s::DEFAULT_KEEP_ALIVE, b"_defaultKeepAlive", t);
    put(
        s::USE_CHUNKED_ENCODING_BY_DEFAULT,
        b"useChunkedEncodingByDefault",
        t,
    );
    put(s::SEND_DATE, b"sendDate", f);
    put(s::REMOVED_CONNECTION, b"_removedConnection", f);
    put(s::REMOVED_CONT_LEN, b"_removedContLen", f);
    put(s::REMOVED_TE, b"_removedTE", f);
    put(s::STRICT_CONTENT_LENGTH, b"strictContentLength", f);
    put(s::CONTENT_LENGTH, b"_contentLength", null);
    put(s::HAS_BODY, b"_hasBody", t);
    let empty = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    put(
        s::TRAILER,
        b"_trailer",
        f64::from_bits(JSValue::string_ptr(empty).bits()),
    );
    put(s::FINISHED, b"finished", f);
    put(s::HEADER_SENT, b"_headerSent", f);
    put(s::CLOSED, b"_closed", f);
    put(s::HEADER, b"_header", null);
    put(s::KEEP_ALIVE_TIMEOUT, b"_keepAliveTimeout", 0.0);
    let nop = scope.root_nanbox_f64(pending_data_nop());
    put(s::ON_PENDING_DATA, b"_onPendingData", nop.get_nanbox_f64());
    let Some(req) = req else {
        return;
    };
    let req = scope.root_nanbox_f64(req);
    // ServerResponse's own.
    let field_of = |target: f64, key: &'static [u8]| {
        if JSValue::from_bits(target.to_bits()).is_reference() {
            crate::runtime_state_key::NamedStateKey {
                bytes: key,
                hash: crate::runtime_state_key::const_key_hash(key),
                memo: None,
            }
            .read_value(target)
        } else {
            undefined()
        }
    };
    let method = field_of(req.get_nanbox_f64(), b"method");
    let head = string_text(method).is_some_and(|m| m == "HEAD");
    if head {
        put(s::HAS_BODY, b"_hasBody", f);
    }
    put(s::REQ, b"req", req.get_nanbox_f64());
    put(s::SEND_DATE, b"sendDate", t);
    put(s::SENT100, b"_sent100", f);
    put(s::EXPECT_CONTINUE, b"_expect_continue", f);
    let major = field_of(req.get_nanbox_f64(), b"httpVersionMajor");
    let minor = field_of(req.get_nanbox_f64(), b"httpVersionMinor");
    let below_one = |v: f64| {
        let j = JSValue::from_bits(v.to_bits());
        (j.is_int32() && j.as_int32() < 1)
            || (j.is_number() && !j.is_int32() && j.as_number() < 1.0)
    };
    if below_one(major) || below_one(minor) {
        let headers = field_of(req.get_nanbox_f64(), b"headers");
        let te = field_of(headers, b"te");
        let chunked = string_text(te).is_some_and(|te| {
            te.to_ascii_lowercase()
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|word| word == "chunked")
        });
        put(
            s::USE_CHUNKED_ENCODING_BY_DEFAULT,
            b"useChunkedEncodingByDefault",
            bool_value(chunked),
        );
        put(s::SHOULD_KEEP_ALIVE, b"shouldKeepAlive", f);
    }
}

/// The text of a string value (heap or inline), `None` for anything else.
fn string_text(value: f64) -> Option<String> {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_string() {
        let ptr = v.as_string_ptr();
        if ptr.is_null() {
            return None;
        }
        // SAFETY: a string value names a live string header and its payload.
        let bytes = unsafe {
            std::slice::from_raw_parts(crate::string::string_data(ptr), (*ptr).byte_len as usize)
        };
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    if v.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut buf);
        return Some(String::from_utf8_lossy(&buf[..n]).into_owned());
    }
    None
}

/// `new ServerResponse(req)`: a fresh response whose prototype is `proto`,
/// born on the response layout when `proto` is the sealed, unwritten
/// `ServerResponse.prototype` (`instance_birth::alloc_on_layout`), else an
/// ordinary object; then node's constructor fields. The native payload is
/// attached by the caller.
///
/// # Safety
/// FFI entry: `proto` and `req` are live JS values.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_server_response_birth(proto: f64, req: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let req = scope.root_nanbox_f64(req);
    let proto = if JSValue::from_bits(proto.to_bits()).is_pointer() {
        proto
    } else {
        ensure_http_prototype(HttpProto::ServerResponse)
    };
    let proto = scope.root_nanbox_f64(proto);
    let obj = match crate::node_stream::instance_birth::alloc_on_layout(
        proto.get_nanbox_f64(),
        &raw const layouts::SERVER_RESPONSE,
    ) {
        Some(obj) => boxed(obj),
        None => crate::object::js_object_create(proto.get_nanbox_f64()),
    };
    let obj = scope.root_nanbox_f64(obj);
    js_node_http_server_response_init(obj.get_nanbox_f64(), req.get_nanbox_f64());
    obj.get_nanbox_f64()
}

#[cfg(test)]
mod tests {
    //! A direct `new ServerResponse(req)` is born on the response layout only
    //! while every prototype of its chain is unwritten: a compiled store to a
    //! sealed http prototype sets its shadow bit, and the next birth takes the
    //! ordinary construction, whose fields are ordinary `[[Set]]`s.
    use crate::object::ObjectHeader;
    use crate::value::{POINTER_MASK, POINTER_TAG, STRING_TAG};

    fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(body)
            .expect("spawn http-prototype test thread")
            .join()
            .expect("http-prototype test thread panicked");
    }

    unsafe fn atom(key: &str) -> u64 {
        let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
        let a = crate::string::js_string_pool_atom(key.as_ptr(), key.len() as u32, hash, 0);
        STRING_TAG | a as u64
    }

    unsafe fn get(obj: usize, key: &str) -> u64 {
        let boxed = f64::from_bits(POINTER_TAG | obj as u64);
        crate::proxy::js_reflect_get(boxed, f64::from_bits(atom(key)), boxed).to_bits()
    }

    unsafe fn on_layout(obj: usize) -> bool {
        crate::node_stream::instance_birth::instance_layout(obj as *const ObjectHeader)
            == Some(&raw const crate::object::runtime_layouts::SERVER_RESPONSE)
    }

    #[test]
    fn a_compiled_store_to_an_http_prototype_ends_layout_births() {
        on_a_fresh_thread(|| unsafe {
            let _no_move = crate::gc::GcSuppressScope::new();
            let proto = super::ensure_http_prototype(super::HttpProto::ServerResponse);
            assert_eq!(proto.to_bits() & !POINTER_MASK, POINTER_TAG);
            let sr = (proto.to_bits() & POINTER_MASK) as usize;
            let om = crate::object::runtime_layouts::node_prototype_of_serial(
                crate::closure::shape::INTRINSIC_SERIAL_OUTGOING_MESSAGE_PROTOTYPE,
            )
            .expect("OutgoingMessage.prototype is built before ServerResponse's")
                as usize;
            let req = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
            let birth = || {
                let v = super::js_node_http_server_response_birth(proto, req);
                (v.to_bits() & POINTER_MASK) as usize
            };
            let word = |p: usize| (*(p as *const ObjectHeader)).parent_class_id;

            // Pristine chain: born on the layout, node's fields in place.
            let a = birth();
            assert!(
                on_layout(a),
                "a pristine birth is born on the response layout"
            );
            assert_eq!(get(a, "statusCode"), 200.0f64.to_bits());
            assert_eq!(get(a, "finished"), crate::value::TAG_FALSE);
            for p in [sr, om] {
                assert!(!crate::object::layout_desc::word_is_shadowed(word(p)));
            }

            // A compiled plain store of a new key on OutgoingMessage.prototype
            // sets its shadow bit; the next birth is ordinary, and reads it.
            crate::object::js_object_set_field_by_name(
                om as *mut ObjectHeader,
                (atom("finished") & POINTER_MASK) as *const crate::StringHeader,
                7.0,
            );
            assert!(
                crate::object::layout_desc::word_is_shadowed(word(om)),
                "a program store to a sealed http prototype sets its shadow bit"
            );
            let b = birth();
            assert!(!on_layout(b), "a birth under a written chain is ordinary");
            assert_eq!(get(b, "finished"), crate::value::TAG_FALSE);
            assert_eq!(get(b, "statusCode"), 200.0f64.to_bits());
        });
    }
}

#[cfg(test)]
mod incoming_init_tests {
    use super::*;

    #[test]
    fn incoming_initializer_fills_readable_state_and_socket_options() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        let read = |object, bytes: &'static [u8]| {
            crate::runtime_state_key::NamedStateKey {
                bytes,
                hash: crate::runtime_state_key::const_key_hash(bytes),
                memo: None,
            }
            .read_value(object)
        };
        unsafe {
            crate::object::native_module_registry::js_nm_install_stream();
            let receiver = boxed(crate::object::js_object_alloc(0, 0));
            let proto = crate::node_stream::stream_prototype_value("Readable");
            crate::object::prototype_chain::object_set_static_prototype(
                (receiver.to_bits() & crate::value::POINTER_MASK) as usize,
                proto.to_bits(),
            );
            let socket = boxed(crate::object::js_object_alloc(0, 1));
            crate::runtime_state_key::js_runtime_state_key_set(
                socket,
                b"readableHighWaterMark".as_ptr(),
                21,
                1234.0,
            );
            js_node_http_incoming_message_init(receiver, socket);
            let state = read(receiver, b"_readableState");
            assert!(JSValue::from_bits(state.to_bits()).is_pointer());
            assert_eq!(read(state, b"highWaterMark"), 1234.0);
            assert_eq!(
                read(state, b"readingMore").to_bits(),
                crate::value::TAG_TRUE
            );
            assert_eq!(read(receiver, b"socket").to_bits(), socket.to_bits());
            assert_eq!(read(receiver, b"client").to_bits(), socket.to_bits());
            assert_eq!(
                read(receiver, b"complete").to_bits(),
                crate::value::TAG_FALSE
            );
        }
    }
}
