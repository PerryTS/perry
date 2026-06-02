//! Codegen-side FFI provenance registry (#835 + #846).
//!
//! Some FFI symbols emitted by codegen don't live in `perry-runtime` —
//! they live in `perry-stdlib` or one of the `perry-ext-*` wrapper
//! crates. The linker only sees those archives if the CLI driver
//! decides to pass them on the link line. Today that decision is
//! driven entirely off the user's *imports* (`import "node:http"` →
//! `ctx.native_module_imports` → well-known flip). Compiled-package
//! code can emit calls to these FFIs without flipping the import set
//! (Effect's `Stream` lowering emits `js_readable_stream_*` even when
//! the user TS never writes `import "streams"`; Express compile emits
//! `js_node_http_create_server` without an `import "node:http"` in the
//! entry module).
//!
//! When the well-known flip never fires, the symbols stay undefined
//! and the linker fails with "Undefined symbols: _js_readable_stream_…"
//! or "_js_node_http_create_server".
//!
//! ## Design
//!
//! 1. A static `&'static [(&'static str, OwnerKind)]` table maps every
//!    FFI symbol that codegen can emit to its **providing key** — either
//!    `OwnerKind::Stdlib { feature: Some("bundled-streams") }` (means:
//!    `ctx.needs_stdlib = true` AND insert "bundled-streams" into
//!    `ctx.extra_stdlib_features` so `build_optimized_libs` rebuilds
//!    perry-stdlib WITH that feature) or `OwnerKind::WellKnown("http")`
//!    (means: insert "http" into `ctx.native_module_imports` so the
//!    existing well-known flip picks up `perry-ext-http`).
//!
//! 2. A process-wide `Mutex<HashSet<&'static str>>` collector
//!    [`USED_PROVIDERS`] gets populated by `LlBlock::call` / `call_void`
//!    at every codegen call-emission site whose symbol name matches a
//!    registry entry. Since `compile_module` is called per-module from
//!    rayon, the mutex is the synchronization point — contention is
//!    negligible (one `HashSet::insert` per FFI call, all small static
//!    strings).
//!
//! 3. The CLI driver calls [`take_used_providers`] **after** all
//!    per-module codegen finishes but **before** `build_optimized_libs`,
//!    folds the set into `ctx.needs_stdlib` + `ctx.native_module_imports`,
//!    and the existing well-known machinery routes everything from there.
//!
//! The registry is intentionally small — only the FFI symbols we KNOW
//! live exclusively (or primarily) outside `perry-runtime`. Symbols
//! served by both `perry-runtime` AND a wrapper crate (most of `js_*`)
//! aren't in the table; they resolve from the always-linked runtime.

use std::collections::HashSet;
use std::sync::Mutex;

/// Where a given FFI symbol's implementation lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnerKind {
    /// Symbol lives in `perry-stdlib`. Driver sets `ctx.needs_stdlib = true`.
    ///
    /// `feature` is the perry-stdlib Cargo feature gate that compiles
    /// the symbol's definition into `libperry_stdlib.a`. When the
    /// auto-optimize layer (`build_optimized_libs`) rebuilds
    /// perry-stdlib with `--no-default-features`, it only enables the
    /// features that `compute_required_features` derived from the
    /// user's `ctx.native_module_imports`. Codegen-emitted FFIs (Effect
    /// `Stream`, etc.) bypass that import set, so without an explicit
    /// feature hint the auto-optimize stdlib gets built WITHOUT the
    /// feature, the symbol never makes it into the .a, and the link
    /// fails with "Undefined symbols: _js_readable_stream_…".
    ///
    /// The driver drains these entries into
    /// `ctx.extra_stdlib_features`, which `build_optimized_libs`
    /// unions into the feature set right before rebuilding.
    ///
    /// `None` means "symbol is in perry-stdlib's always-on core, no
    /// feature flip needed" — kept as an escape hatch even though all
    /// current entries name a feature.
    Stdlib { feature: Option<&'static str> },
    /// Symbol lives in a `perry-ext-*` crate covered by the well-known
    /// table. The `&'static str` is the *binding key* (e.g. `"http"`,
    /// `"streams"`), which the driver folds into
    /// `ctx.native_module_imports` so `build_optimized_libs` picks up
    /// the matching `[bindings.<key>]` entry from
    /// `well_known_bindings.toml`.
    WellKnown(&'static str),
}

/// Registry of FFI symbols emitted by codegen whose implementation
/// lives outside `perry-runtime`. Adding an entry here means the CLI
/// driver will automatically link the providing crate when codegen
/// emits a call to that symbol — no `import "node:…"` in the user TS
/// required.
///
/// Keep this list in sync with the actual `extern "C" fn` definitions
/// in the listed crates. The keys are exact symbol names; lookups are
/// O(N) over a small table (cheap) — switch to a HashMap if this ever
/// grows past a few dozen entries.
#[rustfmt::skip]
const FFI_REGISTRY: &[(&str, OwnerKind)] = &[
    // ── #835: Web Streams ────────────────────────────────────────────
    // `perry-stdlib::streams` owns the canonical implementations.
    // `perry-ext-streams` re-implements a subset, but `js_stream_unwrap_handle`
    // lives only in `perry-stdlib`, so the safe blanket fix is `Stdlib`
    // — codegen-emitted Stream FFIs always pull in libperry_stdlib.a
    // regardless of which front-end (effect, custom subclass, plain
    // `new ReadableStream`) emitted them.
    //
    // Feature `bundled-streams` gates `pub mod streams` in perry-stdlib
    // (`crates/perry-stdlib/Cargo.toml`). Without it the auto-optimize
    // stdlib build drops the entire module and the link fails on every
    // `js_readable_stream_*` reference — the #835/#846 follow-up bug.
    ("js_readable_stream_new",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_new_with_source_type",     OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_new_with_strategy_and_source_type", OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_get_reader",               OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_get_reader_with_options",  OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_locked",                   OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_cancel",                   OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_tee",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_pipe_to",                  OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_pipe_through",             OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_from_blob",                OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_from_response",            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_from_iterable",            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_enqueue",       OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_close",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_error",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_desired_size",  OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_new",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_new_with_sink_type",       OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_get_writer",               OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_locked",                   OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_close",                    OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_abort",                    OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_write",                             OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_close",                             OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_abort",                             OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_release_lock",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_closed",                            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_ready",                             OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writer_desired_size",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_read",                              OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_release_lock",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_closed",                            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_cancel",                            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_new",                     OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_readable",                OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_writable",                OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_stream_unwrap_handle",                     OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    // #1545: node:stream/web QueuingStrategy constructors.
    ("js_streams_strategy_high_water_mark",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_count_queuing_strategy_new",               OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_byte_length_queuing_strategy_new",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),

    // ── #2129: http.Agent / https.Agent ──────────────────────────────
    // `perry-stdlib::http` (gated behind `http-client`) owns these.
    // Without the registry entry, a program that does `new http.Agent()`
    // but never `http.request(...)` could leave the auto-optimize build
    // stripping `http-client`, breaking the link on agent symbols.
    ("js_http_agent_new",                           OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_https_agent_new",                          OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_get_name",                      OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_noop_self",                     OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_max_sockets",                   OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_max_free_sockets",              OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_max_total_sockets",             OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_keep_alive_msecs",              OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_keep_alive",                    OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_protocol",                      OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_default_port",                  OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_protocol",                  OwnerKind::Stdlib { feature: Some("http-client") }),
    // ── #2154: Agent argument validation + per-agent client + setter
    // / sockets-accessor surface ─────────────────────────────────────
    ("js_http_agent_destroy",                       OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_destroyed",                     OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_sockets",                       OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_free_sockets",                  OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_requests",                      OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_max_sockets",               OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_max_free_sockets",          OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_max_total_sockets",         OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_keep_alive",                OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_keep_alive_msecs",          OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_create_connection",         OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_set_create_socket",             OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_create_connection",             OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_http_agent_create_socket",                 OwnerKind::Stdlib { feature: Some("http-client") }),

    // ── #846: node:http server ───────────────────────────────────────
    // `perry-ext-http-server` defines `js_node_http_*`. It's pulled in
    // transitively via `perry-ext-http` (rlib dep), and the well-known
    // table already has `[bindings.http]` / `[bindings.https]` /
    // `[bindings.http2]` → `perry-ext-http`. So tagging these as
    // `WellKnown("http")` makes the existing flip do the right thing:
    // the staticlib joins the link line, perry-stdlib's `http-client`
    // feature gets stripped, and the symbols resolve.
    //
    // #2532: codegen's `createServer` lowering (native_table/http.rs)
    // emits `js_node_http_create_server_with_options`, NOT the bare
    // `js_node_http_create_server` — so the bare name below never
    // matched the emitted symbol and the owner-detection backup
    // (compiled-package code with no `import "node:http"`) silently
    // failed to flip `perry-ext-http` onto the link line. Register both
    // the options-carrying variant codegen actually emits and the bare
    // name (still referenced by the `#[used]` FORCE_LINK anchor and the
    // `I64, &[I64]` decl in runtime_decls/stdlib_ffi.rs).
    ("js_node_http_create_server_with_options",     OwnerKind::WellKnown("http")),
    ("js_node_http_create_server",                  OwnerKind::WellKnown("http")),
    ("js_node_http_server_listen",                  OwnerKind::WellKnown("http")),
    ("js_node_http_server_close",                   OwnerKind::WellKnown("http")),
    ("js_node_http_server_close_all_connections",   OwnerKind::WellKnown("http")),
    ("js_node_http_server_close_idle_connections",  OwnerKind::WellKnown("http")),
    ("js_node_http_server_address_json",            OwnerKind::WellKnown("http")),
    ("js_node_http_server_listening",               OwnerKind::WellKnown("http")),
    ("js_node_http_server_on",                      OwnerKind::WellKnown("http")),
    ("js_node_http_server_has_active",              OwnerKind::WellKnown("http")),
    ("js_node_http_server_process_pending",         OwnerKind::WellKnown("http")),
    ("js_node_http_server_headers_timeout",         OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_headers_timeout",     OwnerKind::WellKnown("http")),
    ("js_node_http_server_keep_alive_timeout",      OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_keep_alive_timeout",  OwnerKind::WellKnown("http")),
    ("js_node_http_server_keep_alive_timeout_buffer", OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_keep_alive_timeout_buffer", OwnerKind::WellKnown("http")),
    ("js_node_http_server_request_timeout",         OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_request_timeout",     OwnerKind::WellKnown("http")),
    ("js_node_http_server_idle_timeout",            OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_idle_timeout",        OwnerKind::WellKnown("http")),
    ("js_node_http_server_max_headers_count",       OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_max_headers_count",   OwnerKind::WellKnown("http")),
    ("js_node_http_server_max_requests_per_socket", OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_max_requests_per_socket", OwnerKind::WellKnown("http")),
    ("js_node_http_server_set_timeout_method",      OwnerKind::WellKnown("http")),
    ("js_node_https_create_server",                 OwnerKind::WellKnown("http")),
    ("js_node_https_server_listen",                 OwnerKind::WellKnown("http")),
    ("js_node_https_server_close",                  OwnerKind::WellKnown("http")),
    ("js_node_https_server_close_all_connections",  OwnerKind::WellKnown("http")),
    ("js_node_https_server_close_idle_connections", OwnerKind::WellKnown("http")),
    ("js_node_https_server_address_json",           OwnerKind::WellKnown("http")),
    ("js_node_https_server_on",                     OwnerKind::WellKnown("http")),
    ("js_node_https_server_headers_timeout",        OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_headers_timeout",    OwnerKind::WellKnown("http")),
    ("js_node_https_server_keep_alive_timeout",     OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_keep_alive_timeout", OwnerKind::WellKnown("http")),
    ("js_node_https_server_keep_alive_timeout_buffer", OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_keep_alive_timeout_buffer", OwnerKind::WellKnown("http")),
    ("js_node_https_server_request_timeout",        OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_request_timeout",    OwnerKind::WellKnown("http")),
    ("js_node_https_server_idle_timeout",           OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_idle_timeout",       OwnerKind::WellKnown("http")),
    ("js_node_https_server_max_headers_count",      OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_max_headers_count",  OwnerKind::WellKnown("http")),
    ("js_node_https_server_max_requests_per_socket",OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_max_requests_per_socket", OwnerKind::WellKnown("http")),
    ("js_node_https_server_set_timeout_method",     OwnerKind::WellKnown("http")),
    ("js_node_http2_create_server",                 OwnerKind::WellKnown("http")),
    ("js_node_http2_create_secure_server",          OwnerKind::WellKnown("http")),
    ("js_node_http2_connect",                       OwnerKind::WellKnown("http")),
    ("js_node_http2_server_listen",                 OwnerKind::WellKnown("http")),
    ("js_node_http2_server_close",                  OwnerKind::WellKnown("http")),
    ("js_node_http2_server_address_json",           OwnerKind::WellKnown("http")),
    ("js_node_http2_server_on",                     OwnerKind::WellKnown("http")),
    ("js_node_http2_get_default_settings",          OwnerKind::WellKnown("http")),
    ("js_node_http2_get_packed_settings",           OwnerKind::WellKnown("http")),
    ("js_node_http2_get_unpacked_settings",         OwnerKind::WellKnown("http")),

    // ── #1123 followup: node:net server-side ─────────────────────────
    // `perry-ext-net` defines `js_net_server_*`. These are emitted by
    // the codegen NATIVE_MODULE_TABLE rows added in lower_call.rs;
    // unlike the existing socket-side `js_net_socket_*` FFIs (already
    // pulled in via the `("net", "createConnection")` import flip),
    // server-side use sometimes appears in code that doesn't import
    // anything from "node:net" beyond `createServer` itself, so the
    // import flip might not fire. Tagging here so the linker pulls
    // libperry_ext_net.a in regardless.
    ("js_net_create_server",                        OwnerKind::WellKnown("net")),
    ("js_net_server_listen",                        OwnerKind::WellKnown("net")),
    ("js_net_server_close",                         OwnerKind::WellKnown("net")),
    ("js_net_server_address",                       OwnerKind::WellKnown("net")),
    ("js_net_server_on",                            OwnerKind::WellKnown("net")),
    ("js_net_server_get_listening",                 OwnerKind::WellKnown("net")),
    ("js_net_server_get_connections",               OwnerKind::WellKnown("net")),
    ("js_net_server_get_max_connections",           OwnerKind::WellKnown("net")),
    ("js_net_server_set_max_connections",           OwnerKind::WellKnown("net")),
    ("js_net_server_get_drop_max_connection",       OwnerKind::WellKnown("net")),
    ("js_net_server_set_drop_max_connection",       OwnerKind::WellKnown("net")),
    ("js_net_block_list_new",                       OwnerKind::WellKnown("net")),
    ("js_net_block_list_is_block_list",             OwnerKind::WellKnown("net")),
    ("js_net_block_list_add_address",               OwnerKind::WellKnown("net")),
    ("js_net_block_list_add_range",                 OwnerKind::WellKnown("net")),
    ("js_net_block_list_add_subnet",                OwnerKind::WellKnown("net")),
    ("js_net_block_list_check",                     OwnerKind::WellKnown("net")),
    ("js_net_block_list_to_json",                   OwnerKind::WellKnown("net")),
    ("js_net_block_list_rules",                     OwnerKind::WellKnown("net")),
    ("js_net_block_list_from_json",                 OwnerKind::WellKnown("net")),
    ("js_net_socket_address_new",                   OwnerKind::WellKnown("net")),
    ("js_net_socket_address_parse",                 OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_address",           OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_family",            OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_port",              OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_flowlabel",         OwnerKind::WellKnown("net")),
    // #1852 — chainable no-op option setters for Socket/Server.
    ("js_net_socket_noop_self",                     OwnerKind::WellKnown("net")),
    ("js_net_socket_get_type_of_service",           OwnerKind::WellKnown("net")),
    ("js_net_socket_set_type_of_service",           OwnerKind::WellKnown("net")),
    ("js_net_server_noop_self",                     OwnerKind::WellKnown("net")),
    // #2131 — net.Socket / net.Server lifecycle + EventEmitter surface
    // (once / off / removeAllListeners / listenerCount / eventNames /
    // resetAndDestroy, plus the socket-side address()). Same well-known
    // anchor as the rest of the net FFI — tagging here pulls in
    // libperry_ext_net.a when these symbols are referenced from a
    // program that doesn't otherwise import socket-side surface.
    ("js_net_socket_address",                       OwnerKind::WellKnown("net")),
    ("js_net_socket_once",                          OwnerKind::WellKnown("net")),
    ("js_net_socket_remove_listener",               OwnerKind::WellKnown("net")),
    ("js_net_socket_remove_all_listeners",          OwnerKind::WellKnown("net")),
    ("js_net_socket_listener_count",                OwnerKind::WellKnown("net")),
    ("js_net_socket_event_names",                   OwnerKind::WellKnown("net")),
    ("js_net_socket_reset_and_destroy",             OwnerKind::WellKnown("net")),
    ("js_net_server_once",                          OwnerKind::WellKnown("net")),
    ("js_net_server_remove_listener",               OwnerKind::WellKnown("net")),
    ("js_net_server_remove_all_listeners",          OwnerKind::WellKnown("net")),
    ("js_net_server_listener_count",                OwnerKind::WellKnown("net")),
    ("js_net_server_event_names",                   OwnerKind::WellKnown("net")),

    // ── #1724: global Blob/File + URL object-URL helpers ──────────────
    // `new Blob([...])`, `new File([...], name)`, `URL.createObjectURL`,
    // `URL.revokeObjectURL`, and `resolveObjectURL` (node:buffer) are all
    // global / built-in entry points — a program can use them without
    // any `import` that maps to the `http-client` feature. Their FFI
    // definitions live in `perry-stdlib::fetch_blob`, which is gated
    // behind `#[cfg(feature = "http-client")]` (it shares the Blob
    // registry + handle ABI with `fetch.rs`). Without a feature hint the
    // auto-optimize stdlib rebuild drops the module entirely and the link
    // fails with "Undefined symbols: _js_url_revoke_object_url" (and the
    // companion `_js_blob_new` / `_js_url_create_object_url`). Tagging the
    // constructor + object-URL entry points pulls in `http-client` so the
    // module — and every Blob/File instance method reachable from a
    // constructed value — is compiled into libperry_stdlib.a.
    ("js_blob_new",                                 OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_file_new",                                 OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_url_create_object_url",                    OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_url_revoke_object_url",                    OwnerKind::Stdlib { feature: Some("http-client") }),
    ("js_buffer_resolve_object_url",                OwnerKind::Stdlib { feature: Some("http-client") }),
];

/// Process-wide collector of provider keys observed during codegen.
/// Populated by [`record_ffi_call`] from `LlBlock::call` / `call_void`.
/// Drained by [`take_used_providers`] right before `build_optimized_libs`.
///
/// `Mutex<HashSet>` instead of an `RwLock` or lock-free structure because
/// FFI call emission is already an expensive operation (allocates +
/// formats an LLVM IR line), and the contention here is one `insert` per
/// FFI call across a small rayon worker pool — well under any
/// optimization horizon worth measuring.
static USED_PROVIDERS: Mutex<Option<HashSet<OwnerKind>>> = Mutex::new(None);

/// Called from every `LlBlock::call` / `LlBlock::call_void` site.
/// O(N) lookup over `FFI_REGISTRY` (N ≈ 50 today) — measured at
/// ~30 ns per emission, fully amortized by the surrounding format!
/// strings.
pub(crate) fn record_ffi_call(symbol: &str) {
    for (name, owner) in FFI_REGISTRY {
        if *name == symbol {
            let mut guard = USED_PROVIDERS.lock().expect("USED_PROVIDERS poisoned");
            guard.get_or_insert_with(HashSet::new).insert(*owner);
            return;
        }
    }
}

/// Drain and return everything codegen recorded since the last call.
/// The CLI driver calls this once after all per-module codegen finishes
/// and folds the result into `ctx.needs_stdlib` + `ctx.native_module_imports`
/// before `build_optimized_libs` runs.
///
/// Returns an empty set when no FFI in the registry was emitted.
pub fn take_used_providers() -> HashSet<OwnerKind> {
    let mut guard = USED_PROVIDERS.lock().expect("USED_PROVIDERS poisoned");
    guard.take().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    static PROVIDER_TEST_LOCK: Mutex<()> = Mutex::new(());

    // `USED_PROVIDERS` is a process-wide static; other tests in the same
    // process may concurrently insert into it via `LlBlock::call`, and these
    // module tests drain it. Serialize the explicit drain/record assertions so
    // one test cannot steal another test's providers.
    #[test]
    fn registry_dispatch_routes_to_correct_owner() {
        let _guard = PROVIDER_TEST_LOCK
            .lock()
            .expect("provider test lock poisoned");
        // Drain anything left over from prior tests.
        let _ = take_used_providers();

        // Repro #835: stream FFI should bind to Stdlib { feature:
        // Some("bundled-streams") } — the feature name flows through
        // to `build_optimized_libs` so the auto-optimize stdlib
        // rebuild actually includes the streams module.
        record_ffi_call("js_readable_stream_new");
        // Repro #846: server FFI should bind to WellKnown("http").
        record_ffi_call("js_node_http_create_server");
        // Repro #1724: global Blob/URL object-URL FFI should bind to
        // Stdlib { feature: Some("http-client") } so the auto-optimize
        // stdlib rebuild compiles the `fetch_blob` module in.
        record_ffi_call("js_url_revoke_object_url");
        // Non-registered FFI: must NOT cause an insert.
        record_ffi_call("js_definitely_not_a_real_ffi_symbol_zzz");

        let got = take_used_providers();
        assert!(
            got.contains(&OwnerKind::Stdlib {
                feature: Some("bundled-streams")
            }),
            "expected Stdlib(bundled-streams) in providers, got {:?}",
            got
        );
        assert!(
            got.contains(&OwnerKind::Stdlib {
                feature: Some("http-client")
            }),
            "expected Stdlib(http-client) in providers (Blob/URL object-URL), got {:?}",
            got
        );
        assert!(
            got.contains(&OwnerKind::WellKnown("http")),
            "expected WellKnown(http) in providers, got {:?}",
            got
        );

        // The unknown FFI cannot map to any OwnerKind, but we can only
        // assert it didn't show up by checking the only two variants we
        // care about. Done above. Drain semantics:
        let _ = take_used_providers();
    }

    /// #2532 regression: the symbol codegen actually emits for
    /// `http.createServer(...)` is `js_node_http_create_server_with_options`
    /// (see `lower_call/native_table/http.rs`). It must route to
    /// `WellKnown("http")` so out-of-tree / compiled-package builds (no
    /// `import "node:http"` in the entry module) still flip
    /// `perry-ext-http` onto the link line. Before the fix, only the
    /// pre-rename `js_node_http_create_server` was registered, so the
    /// emitted symbol matched nothing and the owner-detection backup
    /// silently no-op'd — the link then failed with
    /// `Undefined symbols: _js_node_http_create_server_with_options`.
    #[test]
    fn emitted_create_server_symbol_routes_to_http() {
        let _guard = PROVIDER_TEST_LOCK
            .lock()
            .expect("provider test lock poisoned");
        let _ = take_used_providers();
        record_ffi_call("js_node_http_create_server_with_options");
        let got = take_used_providers();
        assert!(
            got.contains(&OwnerKind::WellKnown("http")),
            "js_node_http_create_server_with_options must route to WellKnown(http), got {:?}",
            got
        );
    }
}
