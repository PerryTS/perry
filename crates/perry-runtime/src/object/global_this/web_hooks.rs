//! The web-platform `globalThis` member tables, reached through hook slots.
//!
//! Five groups — `URL`/`URLPattern`/`URLSearchParams` (`url`), the text codecs
//! and their streams (`text`), `WebSocket` (`websocket`), `Crypto`/`CryptoKey`/
//! `SubtleCrypto` (`webcrypto`) and the fetch value types (`webfetch`) — have a
//! prototype-method table, a constructor-statics table and an intrinsic
//! factory. `populate_global_this_builtins`, the constructor-statics installer
//! and `[[Construct]]` are live in every program, so they must not name the
//! tables directly or the prebuilt runtime keeps every group (the URL family
//! alone is ~106 KB) in every binary. They call these forwarders instead; the
//! tables are reached only through slots the group's install fills (see
//! `crate::feature_hooks`). The compiler installs a group on its identifier
//! tokens, and the fetch group additionally whenever the program uses `fetch`.
//! An empty slot installs nothing: the constructors themselves stay installed
//! either way, so `typeof URL === "function"` holds and only the reflectable
//! member tables (and the by-value factory) are absent.
//!
//! The `Math`, `JSON`, `Reflect` and `Atomics` namespaces work the same way:
//! their reflectable member tables come from the `global-math` / `global-json`
//! / `global-reflect` / `global-atomics` installs, while the namespace object
//! itself always exists (`typeof JSON === "object"`).

use super::install_static::ConstructorStatics;
use crate::feature_hooks::Hook;
use crate::object::global_this_tables::BuiltinConstructorIdentity;
use crate::object::ObjectHeader;

#[derive(Clone, Copy)]
pub(crate) enum WebGroup {
    Url,
    Text,
    Websocket,
    Webcrypto,
    Webfetch,
}

struct Slots {
    proto_methods: Hook<fn(&str, *mut ObjectHeader)>,
    statics: Hook<fn(&str, &mut ConstructorStatics)>,
    construct: Hook<unsafe fn(BuiltinConstructorIdentity, f64, &[f64]) -> Option<f64>>,
}

impl Slots {
    const fn empty() -> Self {
        Slots {
            proto_methods: Hook::empty(),
            statics: Hook::empty(),
            construct: Hook::empty(),
        }
    }
}

static URL: Slots = Slots::empty();
static TEXT: Slots = Slots::empty();
static WEBSOCKET: Slots = Slots::empty();
static WEBCRYPTO: Slots = Slots::empty();
static WEBFETCH: Slots = Slots::empty();
static MATH_MEMBERS: Hook<fn(*mut ObjectHeader)> = Hook::empty();
static JSON_MEMBERS: Hook<fn(*mut ObjectHeader)> = Hook::empty();
static REFLECT_MEMBERS: Hook<fn(*mut ObjectHeader)> = Hook::empty();
static ATOMICS_MEMBERS: Hook<fn(*mut ObjectHeader)> = Hook::empty();
static URL_PROTOTYPE_ACCESSORS: Hook<fn(*mut ObjectHeader)> = Hook::empty();

fn slots(group: WebGroup) -> &'static Slots {
    match group {
        WebGroup::Url => &URL,
        WebGroup::Text => &TEXT,
        WebGroup::Websocket => &WEBSOCKET,
        WebGroup::Webcrypto => &WEBCRYPTO,
        WebGroup::Webfetch => &WEBFETCH,
    }
}

/// Install `builtin_name`'s prototype members from `group`'s table.
pub(crate) fn proto_methods(group: WebGroup, builtin_name: &str, proto_obj: *mut ObjectHeader) {
    if let Some(install) = slots(group).proto_methods.get() {
        install(builtin_name, proto_obj);
    }
}

/// Install `name`'s constructor statics from `group`'s table.
pub(crate) fn statics(group: WebGroup, name: &str, s: &mut ConstructorStatics) {
    if let Some(install) = slots(group).statics.get() {
        install(name, s);
    }
}

/// `group`'s intrinsic factory for a constructor reached through a value.
///
/// # Safety
/// As `class_registry::construct::builtin::construct`.
pub(crate) unsafe fn construct(
    group: WebGroup,
    name: BuiltinConstructorIdentity,
    func_value: f64,
    args: &[f64],
) -> Option<f64> {
    slots(group)
        .construct
        .get()
        .and_then(|construct| unsafe { construct(name, func_value, args) })
}

/// The WebIDL accessors on `URL.prototype` (`global-url`).
pub(crate) fn url_prototype_accessors(proto: *mut ObjectHeader) {
    if let Some(install) = URL_PROTOTYPE_ACCESSORS.get() {
        install(proto);
    }
}

pub(crate) fn install_url() {
    use super::{install_static as st, proto_methods as pm};
    URL.proto_methods.set(pm::web_url_proto_methods);
    URL.statics.set(st::web_url_statics);
    URL.construct
        .set(crate::object::class_registry::web_url_construct);
    URL_PROTOTYPE_ACCESSORS.set(crate::url::prototype::install_url_prototype_accessors);
}

pub(crate) fn install_text() {
    use super::proto_methods as pm;
    TEXT.proto_methods.set(pm::web_text_proto_methods);
    TEXT.construct
        .set(crate::object::class_registry::web_text_construct);
}

pub(crate) fn install_websocket() {
    use super::proto_methods as pm;
    WEBSOCKET.proto_methods.set(pm::web_websocket_proto_methods);
}

pub(crate) fn install_webcrypto() {
    use super::{install_static as st, proto_methods as pm};
    WEBCRYPTO.proto_methods.set(pm::web_webcrypto_proto_methods);
    WEBCRYPTO.statics.set(st::web_webcrypto_statics);
    WEBCRYPTO
        .construct
        .set(crate::object::class_registry::web_webcrypto_construct);
}

pub(crate) fn install_webfetch() {
    use super::proto_methods as pm;
    WEBFETCH.proto_methods.set(pm::web_webfetch_proto_methods);
    WEBFETCH
        .construct
        .set(crate::object::class_registry::web_webfetch_construct);
}

macro_rules! namespace_members {
    ($($slot:ident, $fwd:ident, $install:ident, $imp:path;)+) => {$(
        /// The reflectable members of the namespace, if its install ran.
        pub(crate) fn $fwd(ns_obj: *mut ObjectHeader) {
            if let Some(install) = $slot.get() {
                install(ns_obj);
            }
        }
        pub(crate) fn $install() {
            $slot.set($imp);
        }
    )+};
}

namespace_members! {
    MATH_MEMBERS, math_members, install_math, super::install_math_namespace;
    JSON_MEMBERS, json_members, install_json, super::install_json_namespace_members;
    REFLECT_MEMBERS, reflect_members, install_reflect, super::install_reflect_namespace_members;
    ATOMICS_MEMBERS, atomics_members, install_atomics, super::install_atomics_namespace_members;
}
