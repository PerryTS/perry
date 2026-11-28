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
            HttpProto::OutgoingMessage => layouts::OUTGOING_MESSAGE_PROTOTYPE::desc(),
            HttpProto::ServerResponse => layouts::SERVER_RESPONSE_PROTOTYPE::desc(),
            HttpProto::IncomingMessage => layouts::INCOMING_MESSAGE_PROTOTYPE::desc(),
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
    if let Some(info) = crate::node_stream::http_incoming::method_info(name) {
        return info;
    }
    if !ptr.is_null()
        && matches!(
            name,
            "setHeader" | "writeHead" | "write" | "end" | "_addHeaderLine"
        )
    {
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
    crate::fn_info!(#[realm(realm_decl! { "node:http.IncomingMessage.prototype._addHeaderLineDistinct", capture = property_key, constructible = true; "node:http.OutgoingMessage.prototype._send", capture = property_key, constructible = true; "node:http.OutgoingMessage.prototype._writeRaw", capture = property_key, constructible = true; "node:http.IncomingMessage.prototype._addHeaderLines", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_incomingmessage_prototype_addheaderlines_metadata); "node:http.IncomingMessage.prototype._dumpAndCloseReadable", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_incomingmessage_prototype_dumpandclosereadable_metadata); "node:http.IncomingMessage.prototype.setTimeout", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_incomingmessage_prototype_settimeout_metadata); "node:http.OutgoingMessage.prototype._finish", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_finish_metadata); "node:http.OutgoingMessage.prototype._flush", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_flush_metadata); "node:http.OutgoingMessage.prototype._flushOutput", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_flushoutput_metadata); "node:http.OutgoingMessage.prototype._implicitHeader", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_implicitheader_metadata); "node:http.OutgoingMessage.prototype._isLenientHeaderValidation", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_islenientheadervalidation_metadata); "node:http.OutgoingMessage.prototype._renderHeaders", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_renderheaders_metadata); "node:http.OutgoingMessage.prototype._storeHeader", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_outgoingmessage_prototype_storeheader_metadata); "node:http.OutgoingMessage.prototype.addTrailers", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_addtrailers_metadata); "node:http.OutgoingMessage.prototype.appendHeader", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_outgoingmessage_prototype_appendheader_metadata); "node:http.OutgoingMessage.prototype.cork", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_cork_metadata); "node:http.OutgoingMessage.prototype.destroy", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_destroy_metadata); "node:http.OutgoingMessage.prototype.flushHeaders", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_flushheaders_metadata); "node:http.OutgoingMessage.prototype.getHeader", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_getheader_metadata); "node:http.OutgoingMessage.prototype.getHeaderNames", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_getheadernames_metadata); "node:http.OutgoingMessage.prototype.getHeaders", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_getheaders_metadata); "node:http.OutgoingMessage.prototype.getRawHeaderNames", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_getrawheadernames_metadata); "node:http.OutgoingMessage.prototype.hasHeader", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_hasheader_metadata); "node:http.OutgoingMessage.prototype.pipe", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_pipe_metadata); "node:http.OutgoingMessage.prototype.removeHeader", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_removeheader_metadata); "node:http.OutgoingMessage.prototype.setHeaders", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_outgoingmessage_prototype_setheaders_metadata); "node:http.OutgoingMessage.prototype.setTimeout", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_outgoingmessage_prototype_settimeout_metadata); "node:http.OutgoingMessage.prototype.uncork", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_outgoingmessage_prototype_uncork_metadata); "node:http.ServerResponse.prototype._finish", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_serverresponse_prototype_finish_metadata); "node:http.ServerResponse.prototype._implicitHeader", capture = property_key, constructible = true, spec = {length = 0}, emit = metadata(realm_node_http_serverresponse_prototype_implicitheader_metadata); "node:http.ServerResponse.prototype.assignSocket", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_assignsocket_metadata); "node:http.ServerResponse.prototype.detachSocket", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_detachsocket_metadata); "node:http.ServerResponse.prototype.writeContinue", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_writecontinue_metadata); "node:http.ServerResponse.prototype.writeEarlyHints", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_serverresponse_prototype_writeearlyhints_metadata); "node:http.ServerResponse.prototype.writeInformation", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_node_http_serverresponse_prototype_writeinformation_metadata); "node:http.ServerResponse.prototype.writeProcessing", capture = property_key, constructible = true, spec = {length = 1}, emit = metadata(realm_node_http_serverresponse_prototype_writeprocessing_metadata); "node:http.IncomingMessage.prototype[\"@runtime:kAttachAbortSignal\"]", capture = public_name, constructible = true, spec = {name = "kAttachAbortSignal", length = 0}, emit = metadata(realm_http_symbol_attach_public); "node:http.IncomingMessage.prototype[\"@runtime:kDetachAbortSignal\"]", capture = public_name, constructible = true, spec = {name = "kDetachAbortSignal", length = 0}, emit = metadata(realm_http_symbol_detach_public); "node:http.OutgoingMessage.prototype[\"@runtime:nodejs.rejection\"]", capture = public_name, constructible = true, spec = {name = "destroy", length = 0}, emit = metadata(realm_http_symbol_rejection_public); })] export crate::object::node_http_prototypes::js_node_http_method_fallback, 3; with_declared(3))
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
    let p = || proto.with_mut_ptr::<ObjectHeader, _>(|proto_ptr| proto_ptr);
    // SAFETY (every fill below): the fresh prototype born on `layout`, not yet
    // reachable by program code.
    let fill = |name: &str, value: f64| unsafe { layout.data(p(), name.as_bytes(), value) };
    fill("constructor", constructor.get_nanbox_f64());
    // The three fill closures are inlined, as they were before the stream
    // slots moved the inliner's choices in this codegen unit.
    let method = #[inline(always)] |name: &'static str, length: u32| {
        let value = named_closure(&scope, method_info(name), name, length);
        fill(name, value);
    };
    let accessor = #[inline(always)] |name: &'static str, setter: bool| {
        let get = named_closure(&scope, im_getter_info(name).unwrap_or(crate::fn_info!(#[realm(realm_decl! { "node:http.IncomingMessage.prototype.headersDistinct.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headersdistinct_get_name_metadata); "node:http.IncomingMessage.prototype.trailersDistinct.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailersdistinct_get_name_metadata); "node:http.OutgoingMessage.prototype.closed.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_closed_get_name_metadata); "node:http.OutgoingMessage.prototype.connection.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_connection_get_name_metadata); "node:http.OutgoingMessage.prototype.errored.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_errored_get_name_metadata); "node:http.OutgoingMessage.prototype.headersSent.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_headerssent_get_name_metadata); "node:http.OutgoingMessage.prototype.socket.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_socket_get_name_metadata); "node:http.OutgoingMessage.prototype.writableCorked.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablecorked_get_name_metadata); "node:http.OutgoingMessage.prototype.writableEnded.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writableended_get_name_metadata); "node:http.OutgoingMessage.prototype.writableFinished.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablefinished_get_name_metadata); "node:http.OutgoingMessage.prototype.writableHighWaterMark.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablehighwatermark_get_name_metadata); "node:http.OutgoingMessage.prototype.writableLength.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writablelength_get_name_metadata); "node:http.OutgoingMessage.prototype.writableNeedDrain.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writableneeddrain_get_name_metadata); "node:http.OutgoingMessage.prototype.writableObjectMode.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_writableobjectmode_get_name_metadata); })] export crate::object::node_http_prototypes::http_getter_thunk, 0; with_flags(crate::closure::FN_NON_CONSTRUCTOR)), name, 0);
        let get = scope.root_nanbox_f64(get);
        let set = if setter {
            named_closure(&scope, crate::fn_info!(#[realm(realm_decl! { "node:http.IncomingMessage.prototype.connection.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_connection_get_name_metadata); "node:http.IncomingMessage.prototype.headers.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headers_get_name_metadata); "node:http.IncomingMessage.prototype.headersDistinct.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headersdistinct_get_name_metadata); "node:http.IncomingMessage.prototype.trailers.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailers_get_name_metadata); "node:http.IncomingMessage.prototype.trailersDistinct.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailersdistinct_get_name_metadata); "node:http.OutgoingMessage.prototype.connection.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_connection_get_name_metadata); "node:http.OutgoingMessage.prototype.socket.<set>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_outgoingmessage_prototype_socket_get_name_metadata); })] export crate::object::node_http_prototypes::http_setter_thunk, 1; with_flags(crate::closure::FN_NON_CONSTRUCTOR)), name, 1).to_bits()
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
    let symbol = #[inline(always)] |atom: *const crate::object::layout_desc::StaticAtom, name: &'static str, length: u32| {
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
    // SAFETY: a live ordinary object.
    unsafe { crate::native_payload::payload_cell_of_object(obj) }.map(|_| receiver)
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
        let Some(cell) = crate::native_payload::payload_cell_of_object(obj) else {
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

/// The method a native-handle response inherits under `name`: the function
/// object `ServerResponse.prototype` (or, for a bare `OutgoingMessage`,
/// `OutgoingMessage.prototype`) holds, own or from its parent prototype, so
/// `res.writeHead === ServerResponse.prototype.writeHead` as in node.
/// Calling it runs the native method on its `this`
/// ([`js_node_http_method_fallback`] for a handle), never the receiver's own
/// replacement. `undefined` when neither prototype defines `name` as a method.
#[no_mangle]
pub extern "C" fn js_node_http_response_prototype_method(
    outgoing_only: i32,
    name_ptr: *const u8,
    name_len: usize,
) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return undefined();
    }
    // SAFETY: the caller passes a live name byte range.
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let chain: &[HttpProto] = if outgoing_only != 0 {
        &[HttpProto::OutgoingMessage]
    } else {
        &[HttpProto::ServerResponse, HttpProto::OutgoingMessage]
    };
    for &kind in chain {
        let proto = ensure_http_prototype(kind);
        let holder = JSValue::from_bits(proto.to_bits());
        if !holder.is_pointer() {
            return undefined();
        }
        // SAFETY: a live sealed prototype; its method slots are data slots,
        // so no getter runs.
        let Some(value) =
            (unsafe { crate::object::own_property_get_by_bytes(holder.as_pointer(), name, proto) })
        else {
            continue;
        };
        let bits = value.bits();
        let is_method = JSValue::from_bits(bits).is_pointer()
            && crate::closure::is_closure_ptr((bits & crate::value::POINTER_MASK) as usize);
        return if is_method {
            f64::from_bits(bits)
        } else {
            undefined()
        };
    }
    undefined()
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
        // IncomingMessage's getters are their own bodies (`im_headers_get`, ...).
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

#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_CONNECTION_GET_INFO = crate::object::node_http_prototypes::im_connection_get, 0; with_flags(crate::closure::FN_NON_CONSTRUCTOR)); realm { "node:http.IncomingMessage.prototype.connection.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_connection_get_name_metadata); } }
#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_HEADERS_GET_INFO = crate::object::node_http_prototypes::im_headers_get, 0; with_flags(crate::closure::FN_NON_CONSTRUCTOR)); realm { "node:http.IncomingMessage.prototype.headers.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_headers_get_name_metadata); } }
#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_SIGNAL_GET_INFO = crate::object::node_http_prototypes::im_signal_get, 0; with_flags(crate::closure::FN_NON_CONSTRUCTOR)); realm { "node:http.IncomingMessage.prototype.signal.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_signal_get_name_metadata); } }
#[rustfmt::skip] crate::realm_decl! { info crate::fn_info!(export static IM_TRAILERS_GET_INFO = crate::object::node_http_prototypes::im_trailers_get, 0; with_flags(crate::closure::FN_NON_CONSTRUCTOR)); realm { "node:http.IncomingMessage.prototype.trailers.<get>", capture = property_key, spec = {name = property_key}, emit = metadata(realm_node_http_incomingmessage_prototype_trailers_get_name_metadata); } }

/// `IncomingMessage.prototype`'s own getters, each its own body. A heap
/// message answers as node's getter does; a numeric handle (an HTTP/2
/// compat request) keeps the handle's property dispatch.
fn im_getter_info(name: &str) -> Option<*const crate::closure::JsFunctionInfo> {
    let info: &crate::closure::JsFunctionInfo = match name {
        "connection" => &IM_CONNECTION_GET_INFO,
        "headers" => &IM_HEADERS_GET_INFO,
        "signal" => &IM_SIGNAL_GET_INFO,
        "trailers" => &IM_TRAILERS_GET_INFO,
        _ => return None,
    };
    Some(info as *const _)
}

fn heap_receiver(receiver: f64) -> bool {
    !JSValue::from_bits(receiver.to_bits()).is_handle()
        && crate::object::field_get_set::runtime_read_site::object_receiver(receiver).is_some()
}

/// node's `headers` getter: `this[kHeaders]`, built once.
extern "C" fn im_headers_get(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let receiver = this.as_f64();
    if heap_receiver(receiver) {
        return message_header_view(receiver, HeaderView::Headers);
    }
    http_getter_thunk(closure, this)
}

/// node's `trailers` getter: `this[kTrailers]`, built once.
extern "C" fn im_trailers_get(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let receiver = this.as_f64();
    if heap_receiver(receiver) {
        return message_header_view(receiver, HeaderView::Trailers);
    }
    http_getter_thunk(closure, this)
}

/// node's `connection` getter: `this.socket`.
extern "C" fn im_connection_get(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    use crate::object::runtime_layouts::incoming_slot as s;
    let receiver = this.as_f64();
    if heap_receiver(receiver) {
        return incoming_field(receiver, s::SOCKET).unwrap_or_else(|| unsafe {
            crate::runtime_state_key::js_runtime_state_key_get(receiver, b"socket".as_ptr(), 6)
        });
    }
    http_getter_thunk(closure, this)
}

/// node's `signal` getter.
extern "C" fn im_signal_get(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let receiver = this.as_f64();
    if heap_receiver(receiver) {
        return message_signal(receiver);
    }
    http_getter_thunk(closure, this)
}

/// Which of node's lazily built IncomingMessage header views.
#[derive(Clone, Copy)]
enum HeaderView {
    Headers,
    Trailers,
}

impl HeaderView {
    fn symbol(self) -> &'static crate::object::layout_desc::StaticAtom {
        match self {
            HeaderView::Headers => &layouts::NP_SYMBOL_K_HEADERS,
            HeaderView::Trailers => &layouts::NP_SYMBOL_K_TRAILERS,
        }
    }
    fn count_symbol(self) -> &'static crate::object::layout_desc::StaticAtom {
        match self {
            HeaderView::Headers => &layouts::NP_SYMBOL_K_HEADERS_COUNT,
            HeaderView::Trailers => &layouts::NP_SYMBOL_K_TRAILERS_COUNT,
        }
    }
    fn slot(self) -> u32 {
        use crate::object::runtime_layouts::incoming_slot as s;
        match self {
            HeaderView::Headers => s::K_HEADERS,
            HeaderView::Trailers => s::K_TRAILERS,
        }
    }
    fn count_slot(self) -> u32 {
        use crate::object::runtime_layouts::incoming_slot as s;
        match self {
            HeaderView::Headers => s::K_HEADERS_COUNT,
            HeaderView::Trailers => s::K_TRAILERS_COUNT,
        }
    }
    fn raw_slot(self) -> u32 {
        use crate::object::runtime_layouts::incoming_slot as s;
        match self {
            HeaderView::Headers => s::RAW_HEADERS,
            HeaderView::Trailers => s::RAW_TRAILERS,
        }
    }
    fn raw(self) -> &'static [u8] {
        match self {
            HeaderView::Headers => b"rawHeaders",
            HeaderView::Trailers => b"rawTrailers",
        }
    }
}

/// node's `IncomingMessage.prototype.headers` (`trailers`) getter on an
/// ordinary message: `this[kHeaders]` when set; otherwise a fresh `{}` stored
/// there, filled by `this._addHeaderLine(raw[n], raw[n + 1], dst)` for each
/// pair below `this[kHeadersCount]`. One object per message, so identity and
/// writes hold across reads.
fn message_header_view(receiver: f64, view: HeaderView) -> f64 {
    // A request on its layout holds the view in its slot: one load.
    if let Some(current) = incoming_field(receiver, view.slot()) {
        if crate::value::js_is_truthy(current) != 0 {
            return current;
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(receiver);
    let get_symbol = |atom: &'static crate::object::layout_desc::StaticAtom, slot: u32| {
        incoming_field(this.get_nanbox_f64(), slot).unwrap_or_else(|| unsafe {
            crate::symbol::js_object_get_symbol_property(
                this.get_nanbox_f64(),
                layouts::runtime_symbol_value(atom),
            )
        })
    };
    let current = get_symbol(view.symbol(), view.slot());
    if crate::value::js_is_truthy(current) != 0 {
        return current;
    }
    let dst = scope.root_nanbox_f64(boxed(crate::object::js_object_alloc(0, 0)));
    if incoming_field(this.get_nanbox_f64(), view.slot()).is_some() {
        incoming_set(
            this.get_nanbox_f64(),
            view.slot(),
            b"",
            dst.get_nanbox_f64(),
        );
    } else {
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                this.get_nanbox_f64(),
                layouts::runtime_symbol_value(view.symbol()),
                dst.get_nanbox_f64(),
            )
        };
    }
    let raw_name = view.raw();
    let src = scope.root_nanbox_f64(
        incoming_field(this.get_nanbox_f64(), view.raw_slot()).unwrap_or_else(|| unsafe {
            crate::runtime_state_key::js_runtime_state_key_get(
                this.get_nanbox_f64(),
                raw_name.as_ptr(),
                raw_name.len(),
            )
        }),
    );
    let mut n = 0.0;
    loop {
        let count = get_symbol(view.count_symbol(), view.count_slot());
        if !(n < crate::builtins::js_number_coerce(count)) {
            break;
        }
        let field = scope.root_nanbox_f64(crate::value::js_dyn_index_get(src.get_nanbox_f64(), n));
        let value = scope.root_nanbox_f64(crate::value::js_dyn_index_get(
            src.get_nanbox_f64(),
            n + 1.0,
        ));
        let args = [
            field.get_nanbox_f64(),
            value.get_nanbox_f64(),
            dst.get_nanbox_f64(),
        ];
        // `this._addHeaderLine(...)`: on a pristine request the function is
        // the one `IncomingMessage.prototype` holds (one own-slot read of
        // the sealed prototype), called directly; anything else looks it up.
        match pristine_add_header_line(this.get_nanbox_f64()) {
            Some(add) => {
                crate::object::property_ops::common::call_with_this_and_args(
                    add,
                    this.get_nanbox_f64(),
                    &args,
                );
            }
            None => unsafe {
                crate::runtime_state_key::js_runtime_state_key_call(
                    this.get_nanbox_f64(),
                    b"_addHeaderLine".as_ptr(),
                    14,
                    args.as_ptr(),
                    args.len(),
                );
            },
        }
        n += 2.0;
    }
    get_symbol(view.symbol(), view.slot())
}

/// The `_addHeaderLine` a [`crate::node_stream::http_incoming::pristine`]
/// request inherits: the value `IncomingMessage.prototype` holds in its own
/// data slot (the chain is unwritten, so it is the runtime's). `None` for any
/// other receiver.
fn pristine_add_header_line(receiver: f64) -> Option<f64> {
    let obj = crate::node_stream::http_incoming::pristine(receiver)?;
    // SAFETY: a live object.
    let proto = unsafe { crate::object::shapes::object_prototype_word(obj) };
    let holder = JSValue::from_bits(proto);
    if !holder.is_pointer() {
        return None;
    }
    // SAFETY: the live, sealed, unwritten prototype; its method slots are
    // data slots, so no getter runs.
    let value = unsafe {
        crate::object::own_property_get_by_bytes(
            holder.as_pointer(),
            b"_addHeaderLine",
            f64::from_bits(proto),
        )
    }?;
    Some(f64::from_bits(value.bits()))
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
    let heap = !JSValue::from_bits(receiver.to_bits()).is_handle()
        && crate::object::field_get_set::runtime_read_site::object_receiver(receiver).is_some();
    if heap {
        // node's setters: `headers`/`trailers` store their view symbol,
        // `connection` is `this.socket`.
        let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
        let view = match name {
            b"headers" => Some(HeaderView::Headers),
            b"trailers" => Some(HeaderView::Trailers),
            _ => None,
        };
        if let Some(view) = view {
            let symbol = layouts::runtime_symbol_value(view.symbol());
            unsafe { crate::symbol::js_object_set_symbol_property(receiver, symbol, value) };
            return undefined();
        }
        if name == b"connection" {
            unsafe {
                crate::runtime_state_key::js_runtime_state_key_set(
                    receiver,
                    b"socket".as_ptr(),
                    6,
                    value,
                )
            };
            return undefined();
        }
    }
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
/// order. On an object born on the request layout (or on its keys under a
/// plain derived chain) each store fills its slot; on any other object each
/// is the ordinary [[Set]], so inherited accessors participate.
///
/// # Safety
/// `this` and `socket` are live JavaScript values.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_incoming_message_init(this: f64, socket: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let socket = scope.root_nanbox_f64(socket);
    incoming_init(&this, &socket, None);
}

/// The values node's parser stores on a server request after its
/// constructor (`parserOnHeadersComplete`, `_addHeaderLines`), each rooted
/// by the caller.
struct Parsed<'a, 's> {
    raw: &'a crate::gc::RuntimeHandle<'s>,
    major: f64,
    minor: f64,
    version: &'a crate::gc::RuntimeHandle<'s>,
    url: &'a crate::gc::RuntimeHandle<'s>,
    method: &'a crate::gc::RuntimeHandle<'s>,
    count: f64,
}

/// node's constructor on `this`. With `parsed`, on a request born on its
/// layout under its sealed, unwritten chain (no accessor or read-only data
/// on the chain can observe an intermediate value), each field is born
/// with the parser's final value: one store per slot, in the constructor's
/// key order. Returns whether `parsed` was written; otherwise the caller
/// performs the parser's stores as node does.
unsafe fn incoming_init(
    this: &crate::gc::RuntimeHandle<'_>,
    socket: &crate::gc::RuntimeHandle<'_>,
    parsed: Option<&Parsed<'_, '_>>,
) -> bool {
    use crate::object::runtime_layouts::incoming_slot as s;
    let scope = crate::gc::RuntimeHandleScope::new();
    let read = |value, bytes: &'static [u8]| {
        crate::runtime_state_key::NamedStateKey {
            bytes,
            hash: crate::runtime_state_key::const_key_hash(bytes),
            memo: None,
        }
        .read_value(value)
    };
    let receiver =
        || crate::object::field_get_set::runtime_read_site::object_receiver(this.get_nanbox_f64());
    // One chain check here, then each store fills its slot while the object
    // stays on the layout's keys.
    let plain_chain = receiver().is_some_and(|obj| {
        crate::node_stream::instance_birth::on_layout_with_plain_chain(
            obj,
            layouts::INCOMING_MESSAGE::WORD,
        )
    });
    // node: `streamOptions = { highWaterMark: socket.readableHighWaterMark }`
    // when there is a socket.
    let high_water_mark = scope.root_nanbox_f64(
        if crate::value::js_is_truthy(socket.get_nanbox_f64()) != 0 {
            read(socket.get_nanbox_f64(), b"readableHighWaterMark")
        } else {
            undefined()
        },
    );
    if plain_chain {
        // `Readable.call(this, opts)` on a receiver whose chain is
        // IncomingMessage.prototype's: a Readable, so node's constructor runs
        // in place, its stores fill the layout's Readable slots, and its
        // state is born from the number the options object would hold.
        crate::node_stream::init_readable_native(
            this.get_nanbox_f64(),
            high_water_mark.get_nanbox_f64(),
        );
    } else {
        let options = if crate::value::js_is_truthy(socket.get_nanbox_f64()) != 0 {
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
        // Reuse the Readable explicit-this path, including its instanceof
        // check: an unrelated receiver gets a fresh discarded Readable, as in
        // Node.
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
    }
    let on_layout = plain_chain && receiver().is_some_and(|obj| on_incoming_layout(obj));
    // `this._readableState.readingMore = true`.
    let state = scope.root_nanbox_f64(match receiver() {
        Some(obj) if on_layout => {
            let bits = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
                .add(crate::object::runtime_layouts::readable_slot::STATE as usize);
            f64::from_bits(if bits == crate::value::TAG_HOLE {
                crate::value::TAG_UNDEFINED
            } else {
                bits
            })
        }
        _ => read(this.get_nanbox_f64(), b"_readableState"),
    });
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
    let one_pass = on_layout && parsed.is_some();
    // Every value is allocated before the first store, so the stores below
    // allocate nothing on the layout path and the object does not move.
    fn fresh_array(scope: &crate::gc::RuntimeHandleScope) -> crate::gc::RuntimeHandle<'_> {
        scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ))
    }
    let raw_headers = match parsed {
        Some(p) if one_pass => scope.root_nanbox_f64(p.raw.get_nanbox_f64()),
        _ => fresh_array(&scope),
    };
    let raw_trailers = fresh_array(&scope);
    let url = match parsed {
        Some(p) if one_pass => scope.root_nanbox_f64(p.url.get_nanbox_f64()),
        _ => scope.root_nanbox_f64(crate::value::nanbox_string_key(
            crate::string::js_string_from_bytes(b"".as_ptr(), 0),
        )),
    };
    let null = f64::from_bits(crate::value::TAG_NULL);
    let f = bool_value(false);
    let (major, minor, version, method, count) = match parsed {
        Some(p) if one_pass => (
            p.major,
            p.minor,
            p.version.get_nanbox_f64(),
            p.method.get_nanbox_f64(),
            p.count,
        ),
        _ => (null, null, null, null, 0.0),
    };
    // The stores below run in the layout's key order, from `socket` on, and
    // the one later key (`_eventsCount`, the first listener's) is absent:
    // each store's later keys are all still holes, so it fills its slot in
    // place (`birth_store`'s decision, made once for the whole sequence).
    let in_order = match receiver() {
        Some(obj) if on_layout => {
            let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            *fields.add(crate::object::runtime_layouts::emitter_slot::EVENTS_COUNT as usize)
                == crate::value::TAG_HOLE
                && *fields.add(s::SOCKET as usize) == crate::value::TAG_HOLE
        }
        _ => false,
    };
    let put = |slot: u32, name: &'static [u8], value: f64| match receiver() {
        Some(obj)
            if on_layout
                && *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
                    .add(slot as usize)
                    == crate::value::TAG_HOLE =>
        {
            if in_order {
                crate::object::store_object_field_slot(obj, slot as usize, value.to_bits())
            } else {
                crate::node_stream::instance_birth::birth_store(obj, slot, value)
            }
        }
        _ => crate::runtime_state_key::js_runtime_state_key_set(
            this.get_nanbox_f64(),
            name.as_ptr(),
            name.len(),
            value,
        ),
    };
    let put_symbol = |slot: u32,
                      atom: &'static crate::object::layout_desc::StaticAtom,
                      value: f64| match receiver() {
        Some(obj)
            if on_layout
                && *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
                    .add(slot as usize)
                    == crate::value::TAG_HOLE =>
        {
            if in_order {
                crate::object::store_object_field_slot(obj, slot as usize, value.to_bits())
            } else {
                crate::node_stream::instance_birth::birth_store(obj, slot, value)
            }
        }
        _ => {
            let symbol = layouts::runtime_symbol_value(atom);
            crate::symbol::js_object_set_symbol_property(this.get_nanbox_f64(), symbol, value);
        }
    };
    put(s::SOCKET, b"socket", socket.get_nanbox_f64());
    put(s::HTTP_VERSION_MAJOR, b"httpVersionMajor", major);
    put(s::HTTP_VERSION_MINOR, b"httpVersionMinor", minor);
    put(s::HTTP_VERSION, b"httpVersion", version);
    put(s::COMPLETE, b"complete", f);
    put(s::RAW_HEADERS, b"rawHeaders", raw_headers.get_nanbox_f64());
    put(
        s::RAW_TRAILERS,
        b"rawTrailers",
        raw_trailers.get_nanbox_f64(),
    );
    put(s::JOIN_DUPLICATE_HEADERS, b"joinDuplicateHeaders", f);
    put(s::ABORTED, b"aborted", f);
    put(s::UPGRADE, b"upgrade", null);
    put(s::URL, b"url", url.get_nanbox_f64());
    put(s::METHOD, b"method", method);
    put(s::STATUS_CODE, b"statusCode", null);
    put(s::STATUS_MESSAGE, b"statusMessage", null);
    put(s::CLIENT, b"client", socket.get_nanbox_f64());
    put(s::CONSUMING, b"_consuming", f);
    put(s::DUMPED, b"_dumped", f);
    use layouts as p;
    put_symbol(s::K_HEADERS, &p::NP_SYMBOL_K_HEADERS, null);
    put_symbol(s::K_HEADERS_COUNT, &p::NP_SYMBOL_K_HEADERS_COUNT, count);
    put_symbol(s::K_TRAILERS, &p::NP_SYMBOL_K_TRAILERS, null);
    put_symbol(s::K_TRAILERS_COUNT, &p::NP_SYMBOL_K_TRAILERS_COUNT, 0.0);
    put_symbol(
        s::K_ABORT_CONTROLLER,
        &p::NP_SYMBOL_K_ABORT_CONTROLLER,
        null,
    );
    put_symbol(
        s::K_ABORT_SIGNAL_SOCKET,
        &p::NP_SYMBOL_K_ABORT_SIGNAL_SOCKET,
        null,
    );
    put_symbol(
        s::K_ABORT_SIGNAL_LISTENER,
        &p::NP_SYMBOL_K_ABORT_SIGNAL_LISTENER,
        null,
    );
    put_symbol(
        s::K_ABORT_SIGNAL_DETACHED,
        &p::NP_SYMBOL_K_ABORT_SIGNAL_DETACHED,
        f,
    );
    one_pass
}

/// Is `obj` on the request layout's keys (born on it, on its keys under a
/// derived chain, or on keys added after them; no shadow bit), so each of
/// the layout's keys names its slot?
///
/// # Safety
/// `obj` is a live object header.
#[inline]
unsafe fn on_incoming_layout(obj: *const ObjectHeader) -> bool {
    // Born on the layout and still on it, or on a shape that descends from
    // it by key adds (`req[sym] = v`), whose record names it as its runtime
    // base: the layout's keys keep their slots. A shadowed word (overflow
    // or attribute variant) declines.
    crate::object::shapes::runtime_base_of_word((*obj).parent_class_id)
        == layouts::INCOMING_MESSAGE::WORD
}

/// The word in `slot` of a request on its layout ([`on_incoming_layout`]),
/// `None` when the receiver is not on it or the slot holds the hole (the key
/// is absent; the caller's ordinary read decides).
fn incoming_field(receiver: f64, slot: u32) -> Option<f64> {
    let obj = crate::object::field_get_set::runtime_read_site::object_receiver(receiver)?;
    // SAFETY: a live object; a layout's slots are its inline storage.
    unsafe {
        if !on_incoming_layout(obj) {
            return None;
        }
        let bits = *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
            .add(slot as usize);
        (bits != crate::value::TAG_HOLE).then_some(f64::from_bits(bits))
    }
}

/// Store `value` as `name` (slot `slot`) of a request: a present data key of
/// its layout is one slot store, anything else the ordinary [[Set]].
fn incoming_set(receiver: f64, slot: u32, name: &'static [u8], value: f64) {
    if incoming_field(receiver, slot).is_some() {
        let obj = crate::object::field_get_set::runtime_read_site::object_receiver(receiver)
            .expect("a live request");
        // SAFETY: the key is present plain data in its slot.
        unsafe { crate::object::store_object_field_slot(obj, slot as usize, value.to_bits()) };
        return;
    }
    // SAFETY: `receiver` is a live value.
    unsafe {
        crate::runtime_state_key::js_runtime_state_key_set(
            receiver,
            name.as_ptr(),
            name.len(),
            value,
        )
    };
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
        layouts::SERVER_RESPONSE::WORD,
    );
    let on_layout = || {
        plain_chain
            && crate::object::field_get_set::runtime_read_site::object_receiver(
                this.get_nanbox_f64(),
            )
            .is_some_and(|obj| {
                crate::node_stream::instance_birth::instance_layout(obj).is_some_and(
                    |desc| unsafe { (*desc).layout_id } == layouts::SERVER_RESPONSE::WORD,
                )
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
        j.is_number() && j.as_number() < 1.0
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
        layouts::SERVER_RESPONSE::desc(),
    ) {
        Some(obj) => boxed(obj),
        None => crate::object::js_object_create(proto.get_nanbox_f64()),
    };
    let obj = scope.root_nanbox_f64(obj);
    js_node_http_server_response_init(obj.get_nanbox_f64(), req.get_nanbox_f64());
    obj.get_nanbox_f64()
}

/// A server request as node's parser makes it, in two steps: the object on
/// `proto` (`IncomingMessage.prototype` when `proto` is not an object), then
/// (after the payload attach) `new IncomingMessage(socket)`'s fields and `parserOnHeadersComplete`'s stores in its order (`httpVersionMajor`,
/// `httpVersionMinor`, `httpVersion`, `url`, `method`) and
/// `_addHeaderLines(rawHeaders, n)` (`rawHeaders`, `this[kHeadersCount]`).
/// The native payload is attached by the caller.
///
/// # Safety
/// FFI entry: every argument is a live JS value; `raw_headers` is an array.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_incoming_message_create(proto: f64) -> f64 {
    let proto = if JSValue::from_bits(proto.to_bits()).is_pointer() {
        proto
    } else {
        ensure_http_prototype(HttpProto::IncomingMessage)
    };
    // Born on the request layout when `proto` is the sealed, unwritten
    // `IncomingMessage.prototype`, as a response is on its own.
    match crate::node_stream::instance_birth::alloc_on_layout(
        proto,
        layouts::INCOMING_MESSAGE::desc(),
    ) {
        Some(obj) => boxed(obj),
        None => crate::object::js_object_create(proto),
    }
}

/// node's constructor and parser fields on `this`, a fresh object from
/// [`js_node_http_incoming_message_create`] whose payload the caller has
/// attached (a stream's state record joins an existing payload cell).
///
/// # Safety
/// FFI entry: every argument is a live JS value; `raw_headers` is an array.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_incoming_message_birth(
    this: f64,
    socket: f64,
    raw_headers: f64,
    major: f64,
    minor: f64,
    version: f64,
    url: f64,
    method: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let raw = scope.root_nanbox_f64(raw_headers);
    let version = scope.root_nanbox_f64(version);
    let url = scope.root_nanbox_f64(url);
    let method = scope.root_nanbox_f64(method);
    let obj = scope.root_nanbox_f64(this);
    use crate::object::runtime_layouts::incoming_slot as s;
    let count = crate::array::js_array_length(
        (raw.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader,
    ) as f64;
    let parsed = Parsed {
        raw: &raw,
        major,
        minor,
        version: &version,
        url: &url,
        method: &method,
        count,
    };
    if incoming_init(&obj, &socket, Some(&parsed)) {
        return obj.get_nanbox_f64();
    }
    let put = |slot: u32, name: &'static [u8], value: f64| {
        incoming_set(obj.get_nanbox_f64(), slot, name, value);
    };
    put(s::HTTP_VERSION_MAJOR, b"httpVersionMajor", major);
    put(s::HTTP_VERSION_MINOR, b"httpVersionMinor", minor);
    put(s::HTTP_VERSION, b"httpVersion", version.get_nanbox_f64());
    put(s::URL, b"url", url.get_nanbox_f64());
    put(s::METHOD, b"method", method.get_nanbox_f64());
    if count > 0.0 {
        put(s::RAW_HEADERS, b"rawHeaders", raw.get_nanbox_f64());
        if incoming_field(obj.get_nanbox_f64(), s::K_HEADERS_COUNT).is_some() {
            incoming_set(obj.get_nanbox_f64(), s::K_HEADERS_COUNT, b"", count);
        } else {
            crate::symbol::js_object_set_symbol_property(
                obj.get_nanbox_f64(),
                layouts::runtime_symbol_value(&layouts::NP_SYMBOL_K_HEADERS_COUNT),
                count,
            );
        }
    }
    obj.get_nanbox_f64()
}

/// node's `IncomingMessage.prototype.signal` getter: `this[kAbortController]`,
/// created on first read, and its signal.
fn message_signal(receiver: f64) -> f64 {
    extern "C" {
        fn js_abort_controller_new() -> *mut ObjectHeader;
        fn js_abort_controller_signal(controller: *mut ObjectHeader) -> *mut ObjectHeader;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(receiver);
    use crate::object::runtime_layouts::incoming_slot as s;
    let mut controller = incoming_field(this.get_nanbox_f64(), s::K_ABORT_CONTROLLER)
        .unwrap_or_else(|| unsafe {
            let key = layouts::runtime_symbol_value(&layouts::NP_SYMBOL_K_ABORT_CONTROLLER);
            crate::symbol::js_object_get_symbol_property(this.get_nanbox_f64(), key)
        });
    if crate::value::js_is_truthy(controller) == 0 {
        controller = boxed(unsafe { js_abort_controller_new() });
        let c = scope.root_nanbox_f64(controller);
        if incoming_field(this.get_nanbox_f64(), s::K_ABORT_CONTROLLER).is_some() {
            incoming_set(
                this.get_nanbox_f64(),
                s::K_ABORT_CONTROLLER,
                b"",
                c.get_nanbox_f64(),
            );
        } else {
            let key = layouts::runtime_symbol_value(&layouts::NP_SYMBOL_K_ABORT_CONTROLLER);
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    this.get_nanbox_f64(),
                    key,
                    c.get_nanbox_f64(),
                )
            };
        }
        controller = c.get_nanbox_f64();
    }
    let ptr = (controller.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
    boxed(unsafe { js_abort_controller_signal(ptr) })
}

/// The request's exchange is over: abort `this[kAbortController]` if a
/// program read `req.signal` (node's `abortIncoming` / `onRequestClose`).
///
/// # Safety
/// FFI entry: `this` is a live JS value.
#[no_mangle]
pub unsafe extern "C" fn js_node_http_incoming_message_abort_signal(this: f64) {
    extern "C" {
        fn js_abort_controller_abort(controller: *mut ObjectHeader);
    }
    if crate::object::field_get_set::runtime_read_site::object_receiver(this).is_none() {
        return;
    }
    let controller = incoming_field(
        this,
        crate::object::runtime_layouts::incoming_slot::K_ABORT_CONTROLLER,
    )
    .unwrap_or_else(|| {
        let key = layouts::runtime_symbol_value(&layouts::NP_SYMBOL_K_ABORT_CONTROLLER);
        crate::symbol::js_object_get_symbol_property(this, key)
    });
    if JSValue::from_bits(controller.to_bits()).is_pointer() {
        js_abort_controller_abort(
            (controller.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader,
        );
    }
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
        crate::node_stream::instance_birth::instance_layout(obj as *const ObjectHeader).is_some_and(
            |desc| (*desc).layout_id == crate::object::runtime_layouts::SERVER_RESPONSE::WORD,
        )
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
    fn no_methods(_: &mut crate::native_payload::PayloadPrototype) {}

    /// A program key add (`res.write = f`) moves the response to a dynamic
    /// shape whose record names the response layout as its runtime base:
    /// the payload is still read from its fixed slot, no key lookup.
    #[test]
    fn a_program_key_add_keeps_the_response_layout_as_runtime_base() {
        on_a_fresh_thread(|| unsafe {
            static FAMILY: crate::native_payload::NativePayloadFamily =
                crate::native_payload::NativePayloadFamily {
                    class_id: crate::native_class_ids::HTTP_SERVER_RESPONSE,
                    links_owner: false,
                    name: "ServerResponse",
                    constructor_export: None,
                    constructor_length: 0,
                    install_prototype: no_methods,
                };
            let _no_move = crate::gc::GcSuppressScope::new();
            let proto = super::ensure_http_prototype(super::HttpProto::ServerResponse);
            let req = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
            let res = super::js_node_http_server_response_birth(proto, req);
            let obj = (res.to_bits() & POINTER_MASK) as usize;
            assert!(crate::native_payload::attach_to_object(
                res, &FAMILY, 7u64, 0
            ));
            let born = (*(obj as *const ObjectHeader)).parent_class_id;
            let cell = crate::native_payload::payload_cell_of_object(obj as *const ObjectHeader);
            assert!(cell.is_some());
            crate::object::js_object_set_field_by_name(
                obj as *mut ObjectHeader,
                (atom("perryAddedKey") & POINTER_MASK) as *const crate::StringHeader,
                3.0,
            );
            let word = (*(obj as *const ObjectHeader)).parent_class_id;
            assert_ne!(word, born, "the add leaves the response layout's word");
            assert_eq!(
                crate::object::shapes::runtime_base_of_word(word),
                crate::object::runtime_layouts::SERVER_RESPONSE::WORD,
                "the added-to shape names the response layout as its base"
            );
            assert_eq!(
                crate::native_payload::payload_cell_of_object(obj as *const ObjectHeader),
                cell,
                "the payload is read from the base layout's slot"
            );
            assert_eq!(get(obj, "perryAddedKey"), 3.0f64.to_bits());
            assert_eq!(get(obj, "statusCode"), 200.0f64.to_bits());
            // An ordinary object with other keys has no base.
            let plain = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
            let p = (plain.to_bits() & POINTER_MASK) as usize;
            crate::object::js_object_set_field_by_name(
                p as *mut ObjectHeader,
                (atom("perryAddedKey") & POINTER_MASK) as *const crate::StringHeader,
                1.0,
            );
            assert_eq!(
                crate::object::shapes::runtime_base_of_word(
                    (*(p as *const ObjectHeader)).parent_class_id
                ),
                0
            );
        });
    }

    /// The response layout lists the payload key: the attach fills its slot,
    /// and the response stays on its layout (no key-add transition).
    #[test]
    fn a_payload_attach_on_the_response_layout_fills_its_listed_slot() {
        on_a_fresh_thread(|| unsafe {
            static FAMILY: crate::native_payload::NativePayloadFamily =
                crate::native_payload::NativePayloadFamily {
                    class_id: crate::native_class_ids::HTTP_SERVER_RESPONSE,
                    links_owner: false,
                    name: "ServerResponse",
                    constructor_export: None,
                    constructor_length: 0,
                    install_prototype: no_methods,
                };
            let _no_move = crate::gc::GcSuppressScope::new();
            let proto = super::ensure_http_prototype(super::HttpProto::ServerResponse);
            let req = crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL));
            let res = super::js_node_http_server_response_birth(proto, req);
            let obj = (res.to_bits() & POINTER_MASK) as usize;
            assert!(
                on_layout(obj),
                "a pristine birth is born on the response layout"
            );
            let word = (*(obj as *const ObjectHeader)).parent_class_id;
            assert!(crate::native_payload::attach_to_object(
                res, &FAMILY, 7u64, 0
            ));
            assert_eq!(
                (*(obj as *const ObjectHeader)).parent_class_id,
                word,
                "the attach keeps the response on its layout"
            );
            let cell = crate::native_payload::payload_cell_of_object(obj as *const ObjectHeader)
                .expect("the attached cell is the payload slot");
            let fields = (obj + std::mem::size_of::<ObjectHeader>()) as *const u64;
            assert_eq!(
                *fields.add(crate::object::runtime_layouts::response_slot::PAYLOAD as usize)
                    & POINTER_MASK,
                cell as u64,
                "the payload is in the slot the layout lists"
            );
            assert_eq!(get(obj, "statusCode"), 200.0f64.to_bits());
            // node's `_maxRequestsPerSocket`, `statusMessage` and
            // `statusCode` stores after the attach: the written payload is a
            // private field with no enumeration order, so each fills its
            // slot in place and the response stays on its layout.
            for (key, value) in [
                ("_maxRequestsPerSocket", 0.0f64),
                ("statusMessage", 1.0),
                ("statusCode", 404.0),
            ] {
                crate::runtime_state_key::js_runtime_state_key_set(
                    res,
                    key.as_ptr(),
                    key.len(),
                    value,
                );
                assert_eq!(get(obj, key), value.to_bits());
            }
            assert_eq!(
                (*(obj as *const ObjectHeader)).parent_class_id,
                word,
                "late member stores after the attach keep the response on its layout"
            );
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
