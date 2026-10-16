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
//! 2. Before codegen, the compile driver computes the program's provider
//!    set from HIR ([`crate::providers::program_providers`], design
//!    GC_EFFECTS_AT_BUILD §4.4) and resolves the link set (auto-optimize
//!    build or prebuilt archives) from it, folding the set into
//!    `ctx.needs_stdlib`, `ctx.extra_stdlib_features` and
//!    `ctx.native_module_imports`.
//!
//! 3. Codegen only checks: `LlBlock::call` / `call_void` record each
//!    emitted registry symbol in a per-module capture
//!    ([`record_ffi_call`]), and the driver rejects a module whose capture
//!    (or cached manifest) names an owner outside the precomputed set
//!    ([`check_emitted_providers`]).
//!
//! The registry is intentionally small — only the FFI symbols we KNOW
//! live exclusively (or primarily) outside `perry-runtime`. Symbols
//! served by both `perry-runtime` AND a wrapper crate (most of `js_*`)
//! aren't in the table; they resolve from the always-linked runtime.
//!
//! ## Prefix net (ext bindings with a `js_<binding>_*` convention)
//!
//! The exact table above is hand-maintained per symbol, which is right for
//! bindings whose symbol names don't map cleanly to a binding key
//! (`js_node_http_*` → `http`). But a large family of data-store / client
//! wrappers name every symbol `js_<binding>_*` 1:1 with the binding key
//! (`js_undici_*` → `perry-ext-undici`). Those are routed generically by
//! [`EXT_PREFIX_REGISTRY`], so an AOT-compiled `undici`/`typescript`
//! (a `perry.compilePackages` member that never appears in any import set)
//! still flips its wrapper onto the link line off the emitted FFI alone.

use std::cell::RefCell;
use std::collections::HashSet;

/// Where a given FFI symbol's implementation lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// The driver folds these entries into
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
    // ── #9604: Bun.SQL ───────────────────────────────────────────────
    // The rest of the `"bun"` module stays runtime-only. Pull SQLite into
    // perry-stdlib only when codegen actually emits the SQL constructor.
    ("js_bun_sql_new",                           OwnerKind::Stdlib { feature: Some("database-sqlite") }),

    // The Bun dispatch bucket can reach listen/connect through extracted
    // callable exports, so installing it also activates the net provider.
    ("js_bun_tcp_nm_install",                       OwnerKind::WellKnown("net")),
    // #10428/#10429: a materialized `net` / `http`/`https`/`http2` namespace
    // or bound export installs its provider's value-form dispatcher.
    ("js_ext_net_nm_install",                       OwnerKind::WellKnown("net")),
    ("js_ext_http_nm_install",                      OwnerKind::WellKnown("http")),
    ("js_ext_zlib_nm_install",                      OwnerKind::WellKnown("zlib")),
    // ── #835: Web Streams ────────────────────────────────────────────
    // `perry-stdlib::streams` owns the only implementation. Codegen-emitted
    // Stream FFIs always pull in libperry_stdlib.a regardless of which
    // front-end (effect, custom subclass, plain `new ReadableStream`) emitted
    // them.
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
    ("js_readable_stream_pipe_through_pair",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_pipe_through_validate",    OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_from_blob",                OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_from_response",            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_from_iterable",            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_new_from_source_object",   OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_enqueue",       OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_close",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_error",         OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_desired_size",  OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_new",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_new_with_sink_type",       OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_writable_stream_new_from_sink_object",     OwnerKind::Stdlib { feature: Some("bundled-streams") }),
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
    // #4915: BYOB reader surface.
    ("js_reader_read_with_view",                    OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_get_byob_reader",          OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_readable_stream_controller_byob_request",  OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_release_lock",                      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_closed",                            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_reader_cancel",                            OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_new",                     OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_new_from_transformer_object", OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_readable",                OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_transform_stream_writable",                OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_stream_unwrap_handle",                     OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_stream_web_text_encoder_stream_new",       OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_stream_web_text_decoder_stream_new",       OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_stream_web_compression_stream_new",        OwnerKind::Stdlib { feature: Some("bundled-streams") }),
    ("js_stream_web_decompression_stream_new",      OwnerKind::Stdlib { feature: Some("bundled-streams") }),
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

    // ── #3954: external node:http client/server surface ─────────────
    // These native-table rows are implemented by `perry-ext-http`
    // rather than `perry-stdlib`. They must flip the `"http"`
    // well-known binding even when compiled-package lowering emits the
    // call without a source-level `import "node:http"` in the entry.
    ("js_http_request_overload",                    OwnerKind::WellKnown("http")),
    ("js_http_get_overload",                        OwnerKind::WellKnown("http")),
    ("js_https_request_overload",                   OwnerKind::WellKnown("http")),
    ("js_https_get_overload",                       OwnerKind::WellKnown("http")),
    ("js_http_request",                             OwnerKind::WellKnown("http")),
    ("js_http_get",                                 OwnerKind::WellKnown("http")),
    ("js_https_request",                            OwnerKind::WellKnown("http")),
    ("js_https_get",                                OwnerKind::WellKnown("http")),
    ("js_http_on",                                  OwnerKind::WellKnown("http")),
    ("js_http_once",                                OwnerKind::WellKnown("http")),
    ("js_http_set_header",                          OwnerKind::WellKnown("http")),
    ("js_http_set_timeout",                         OwnerKind::WellKnown("http")),
    ("js_http_set_timeout_full",                    OwnerKind::WellKnown("http")),
    ("js_http_client_request_end",                  OwnerKind::WellKnown("http")),
    ("js_http_client_request_write",                OwnerKind::WellKnown("http")),
    ("js_http_client_request_end_full",             OwnerKind::WellKnown("http")),
    ("js_http_client_request_write_full",           OwnerKind::WellKnown("http")),
    ("js_http_client_request_listener_count",       OwnerKind::WellKnown("http")),
    ("js_http_client_request_get_header",           OwnerKind::WellKnown("http")),
    ("js_http_client_request_has_header",           OwnerKind::WellKnown("http")),
    ("js_http_client_request_remove_header",        OwnerKind::WellKnown("http")),
    ("js_http_client_request_get_header_names",     OwnerKind::WellKnown("http")),
    ("js_http_client_request_get_headers",          OwnerKind::WellKnown("http")),
    ("js_http_client_request_get_raw_header_names", OwnerKind::WellKnown("http")),
    ("js_http_client_request_abort",                OwnerKind::WellKnown("http")),
    ("js_http_client_request_destroy",              OwnerKind::WellKnown("http")),
    ("js_http_client_request_noop_undefined",       OwnerKind::WellKnown("http")),
    ("js_http_client_request_flush_headers",        OwnerKind::WellKnown("http")),
    ("js_http_client_request_method",               OwnerKind::WellKnown("http")),
    ("js_http_client_request_protocol",             OwnerKind::WellKnown("http")),
    ("js_http_client_request_host",                 OwnerKind::WellKnown("http")),
    ("js_http_client_request_path",                 OwnerKind::WellKnown("http")),
    ("js_http_client_request_aborted",              OwnerKind::WellKnown("http")),
    ("js_http_client_request_destroyed",            OwnerKind::WellKnown("http")),
    ("js_http_client_request_finished",             OwnerKind::WellKnown("http")),
    ("js_http_client_request_reused_socket",        OwnerKind::WellKnown("http")),
    ("js_http_client_request_max_headers_count",    OwnerKind::WellKnown("http")),
    ("js_http_client_request_writable_ended",       OwnerKind::WellKnown("http")),
    ("js_http_client_request_writable_finished",    OwnerKind::WellKnown("http")),
    ("js_http_client_request_socket",               OwnerKind::WellKnown("http")),

    // ── #846: node:http server ───────────────────────────────────────
    // `perry-ext-http` defines `js_node_http_*` in its internal server
    // module, and the well-known
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
    ("js_node_http_outgoing_message_new",           OwnerKind::WellKnown("http")),
    ("js_node_http_server_listen",                  OwnerKind::WellKnown("http")),
    ("js_node_http_server_close",                   OwnerKind::WellKnown("http")),
    ("js_node_http_server_close_all_connections",   OwnerKind::WellKnown("http")),
    ("js_node_http_server_close_idle_connections",  OwnerKind::WellKnown("http")),
    ("js_node_http_server_address_json",            OwnerKind::WellKnown("http")),
    ("js_node_http_server_listening",               OwnerKind::WellKnown("http")),
    ("js_node_http_server_listening_value",         OwnerKind::WellKnown("http")),
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
    ("js_node_http_server_ref",                     OwnerKind::WellKnown("http")),
    ("js_node_http_server_unref",                   OwnerKind::WellKnown("http")),
    ("js_node_http_im_on",                          OwnerKind::WellKnown("http")),
    ("js_node_http_im_once",                        OwnerKind::WellKnown("http")),
    ("js_node_http_im_pause",                       OwnerKind::WellKnown("http")),
    ("js_node_http_im_resume",                      OwnerKind::WellKnown("http")),
    ("js_node_http_im_pause_self",                  OwnerKind::WellKnown("http")),
    ("js_node_http_im_resume_self",                 OwnerKind::WellKnown("http")),
    ("js_node_http_im_destroy",                     OwnerKind::WellKnown("http")),
    ("js_node_http_im_read",                        OwnerKind::WellKnown("http")),
    ("js_node_http_im_set_timeout",                 OwnerKind::WellKnown("http")),
    ("js_node_http_im_method",                      OwnerKind::WellKnown("http")),
    ("js_node_http_im_url",                         OwnerKind::WellKnown("http")),
    ("js_node_http_im_http_version",                OwnerKind::WellKnown("http")),
    ("js_node_http_im_http_version_major",          OwnerKind::WellKnown("http")),
    ("js_node_http_im_http_version_minor",          OwnerKind::WellKnown("http")),
    ("js_node_http_im_complete",                    OwnerKind::WellKnown("http")),
    ("js_node_http_im_aborted",                     OwnerKind::WellKnown("http")),
    ("js_node_http_im_destroyed",                   OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_header_self",            OwnerKind::WellKnown("http")),
    ("js_node_http_res_get_header",                 OwnerKind::WellKnown("http")),
    ("js_node_http_res_remove_header",              OwnerKind::WellKnown("http")),
    ("js_node_http_res_has_header_value",           OwnerKind::WellKnown("http")),
    ("js_node_http_res_get_headers_json",           OwnerKind::WellKnown("http")),
    ("js_node_http_res_get_header_names_json",      OwnerKind::WellKnown("http")),
    ("js_node_http_res_append_header",              OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_headers",                OwnerKind::WellKnown("http")),
    ("js_node_http_res_write_head",                 OwnerKind::WellKnown("http")),
    ("js_node_http_res_write",                      OwnerKind::WellKnown("http")),
    ("js_node_http_res_write_full",                 OwnerKind::WellKnown("http")),
    ("js_node_http_res_add_trailers",               OwnerKind::WellKnown("http")),
    ("js_node_http_res_end",                        OwnerKind::WellKnown("http")),
    ("js_node_http_res_end_full",                   OwnerKind::WellKnown("http")),
    ("js_node_http_res_flush_headers",              OwnerKind::WellKnown("http")),
    ("js_node_http_res_cork",                       OwnerKind::WellKnown("http")),
    ("js_node_http_res_uncork",                     OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_timeout",                OwnerKind::WellKnown("http")),
    ("js_node_http_res_write_early_hints",          OwnerKind::WellKnown("http")),
    ("js_node_http_res_write_continue",             OwnerKind::WellKnown("http")),
    ("js_node_http_res_write_processing",           OwnerKind::WellKnown("http")),
    ("js_node_http_res_on",                         OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_status",                 OwnerKind::WellKnown("http")),
    ("js_node_http_res_get_status",                 OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_status_message",         OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_send_date",              OwnerKind::WellKnown("http")),
    ("js_node_http_res_set_strict_content_length",  OwnerKind::WellKnown("http")),
    ("js_node_http_res_headers_sent",               OwnerKind::WellKnown("http")),
    ("js_node_http_res_writable_ended",             OwnerKind::WellKnown("http")),
    ("js_node_http_res_writable_finished",          OwnerKind::WellKnown("http")),
    ("js_node_https_create_server",                 OwnerKind::WellKnown("http")),
    ("js_node_https_server_listen",                 OwnerKind::WellKnown("http")),
    ("js_node_https_server_close",                  OwnerKind::WellKnown("http")),
    ("js_node_https_server_close_all_connections",  OwnerKind::WellKnown("http")),
    ("js_node_https_server_close_idle_connections", OwnerKind::WellKnown("http")),
    ("js_node_https_server_address_json",           OwnerKind::WellKnown("http")),
    ("js_node_https_server_on",                     OwnerKind::WellKnown("http")),
    ("js_node_https_server_listening_value",        OwnerKind::WellKnown("http")),
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
    ("js_node_https_server_ref",                    OwnerKind::WellKnown("http")),
    ("js_node_https_server_unref",                  OwnerKind::WellKnown("http")),
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
    ("js_ext_net_create_server",                    OwnerKind::WellKnown("net")),
    ("js_ext_net_socket_connect",                   OwnerKind::WellKnown("net")),
    ("js_bun_tcp_listen",                           OwnerKind::WellKnown("net")),
    ("js_bun_tcp_connect",                          OwnerKind::WellKnown("net")),
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
    ("js_net_socket_address_parse_value",           OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_address",           OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_family",            OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_port",              OwnerKind::WellKnown("net")),
    ("js_net_socket_address_get_flowlabel",         OwnerKind::WellKnown("net")),
    // #2013 — net validation rows whose runtime helpers live only in
    // `perry-ext-net`. These calls can be emitted by native-table lowering,
    // so they must independently flip the net well-known provider.
    ("js_net_get_default_auto_select_family",       OwnerKind::WellKnown("net")),
    ("js_net_set_default_auto_select_family",       OwnerKind::WellKnown("net")),
    ("js_net_get_default_auto_select_family_attempt_timeout", OwnerKind::WellKnown("net")),
    ("js_net_set_default_auto_select_family_attempt_timeout", OwnerKind::WellKnown("net")),
    ("js_net_socket_set_timeout",                   OwnerKind::WellKnown("net")),
    // #1852 — chainable no-op option setters for Socket/Server.
    ("js_net_socket_noop_self",                     OwnerKind::WellKnown("net")),
    ("js_net_socket_ref",                           OwnerKind::WellKnown("net")),
    ("js_net_socket_unref",                         OwnerKind::WellKnown("net")),
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
    ("js_ext_net_socket_once",                      OwnerKind::WellKnown("net")),
    ("js_ext_net_socket_on",                        OwnerKind::WellKnown("net")),
    ("js_ext_tls_connect",                          OwnerKind::WellKnown("net")),
    ("js_net_socket_remove_listener",               OwnerKind::WellKnown("net")),
    ("js_net_socket_remove_all_listeners",          OwnerKind::WellKnown("net")),
    ("js_net_socket_listener_count",                OwnerKind::WellKnown("net")),
    ("js_net_socket_event_names",                   OwnerKind::WellKnown("net")),
    ("js_net_socket_listeners",                     OwnerKind::WellKnown("net")),
    ("js_net_socket_raw_listeners",                 OwnerKind::WellKnown("net")),
    ("js_net_socket_reset_and_destroy",             OwnerKind::WellKnown("net")),
    ("js_net_server_once",                          OwnerKind::WellKnown("net")),
    // tokio lane L4: the socket-side core and `tls.connect`. These had no
    // row because perry-stdlib's bundled `net` copy also defined them, so a
    // stdlib-only link resolved them without the wrapper. That copy is
    // deleted; perry-ext-net is the only definition, so an emitted call must
    // put its archive on the link line like every other net symbol.
    ("js_ext_net_socket_alloc",                    OwnerKind::WellKnown("net")),
    ("js_net_socket_alloc",                         OwnerKind::WellKnown("net")),
    ("js_net_socket_connect",                       OwnerKind::WellKnown("net")),
    ("js_net_socket_method_connect",                OwnerKind::WellKnown("net")),
    ("js_net_socket_on",                            OwnerKind::WellKnown("net")),
    ("js_net_socket_read",                          OwnerKind::WellKnown("net")),
    ("js_net_socket_write",                         OwnerKind::WellKnown("net")),
    ("js_net_socket_end",                           OwnerKind::WellKnown("net")),
    ("js_net_socket_destroy",                       OwnerKind::WellKnown("net")),
    ("js_net_socket_upgrade_tls",                   OwnerKind::WellKnown("net")),
    ("js_tls_connect",                              OwnerKind::WellKnown("net")),
    ("js_net_server_remove_listener",               OwnerKind::WellKnown("net")),
    ("js_net_server_remove_all_listeners",          OwnerKind::WellKnown("net")),
    ("js_net_server_listener_count",                OwnerKind::WellKnown("net")),
    ("js_net_server_event_names",                   OwnerKind::WellKnown("net")),

    // HTTP upgrade flows can surface a Perry WebSocket client handle through
    // `node:http` without a source-level `import "ws"`. The codegen receiver
    // rows still emit the ws client helper symbols, so those calls must flip
    // the `ws` well-known wrapper onto the link line independently.
    ("js_ws_send_client_i64",                       OwnerKind::WellKnown("ws")),
    ("js_ws_close_client_i64",                      OwnerKind::WellKnown("ws")),
    ("js_ws_on_client_i64",                         OwnerKind::WellKnown("ws")),
    // #6439: the same "no source-level `import \"ws\"`" situation reaches the
    // REST of the ws surface, not just the three client helpers above. A library
    // that merely *mentions* a socket API gets its module compiled — and its
    // callsites emitted — without any ws import: @effect/platform's `Socket.ts`
    // is pulled in through the package barrel and emits `js_ws_connect_start`,
    // so `perry compile` of any @effect/platform program died with
    //   Undefined symbols: _js_ws_connect_start
    // even though the plain (non-auto-optimize) runtime links it fine.
    //
    // The reason it only breaks under auto-optimize: the definition lives in
    // perry-stdlib behind `bundled-ws`, and the rebuild uses
    // `--no-default-features` + only the features derived from
    // `native_module_imports` — which never contains "ws" here. Meanwhile the
    // runtime's own `ws_stubs` are compiled OUT whenever any perry-ext-* crate
    // is in the graph (they build perry-runtime with `external-ws-symbols`, and
    // cargo unifies features), so nothing defines the symbol at all: stub gone,
    // stdlib feature stripped, perry-ext-ws never linked.
    //
    // Registering the emitted symbols flips the `ws` well-known wrapper onto the
    // link line, which is exactly what a manual `PERRY_FORCE_WELL_KNOWN=ws`
    // does (verified to fix both `api.ts` and `web.ts` of the effect repro).
    // The flip also drops perry-stdlib's `bundled-ws`, so there is no
    // duplicate-symbol risk.
    ("js_ws_connect",                               OwnerKind::WellKnown("ws")),
    ("js_ws_connect_start",                         OwnerKind::WellKnown("ws")),
    ("js_ws_send",                                  OwnerKind::WellKnown("ws")),
    ("js_ws_send_value",                            OwnerKind::WellKnown("ws")),
    ("js_ws_send_value_client_i64",                 OwnerKind::WellKnown("ws")),
    ("js_ws_close",                                 OwnerKind::WellKnown("ws")),
    ("js_ws_close_with",                            OwnerKind::WellKnown("ws")),
    ("js_ws_close_with_client_i64",                 OwnerKind::WellKnown("ws")),
    ("js_ws_ping",                                  OwnerKind::WellKnown("ws")),
    ("js_ws_pong",                                  OwnerKind::WellKnown("ws")),
    ("js_ws_terminate",                             OwnerKind::WellKnown("ws")),
    ("js_ws_on",                                    OwnerKind::WellKnown("ws")),
    ("js_ws_receive",                               OwnerKind::WellKnown("ws")),
    ("js_ws_is_open",                               OwnerKind::WellKnown("ws")),
    ("js_ws_ready_state",                           OwnerKind::WellKnown("ws")),
    ("js_ws_message_count",                         OwnerKind::WellKnown("ws")),
    ("js_ws_wait_for_message",                      OwnerKind::WellKnown("ws")),
    ("js_ws_handle_to_i64",                         OwnerKind::WellKnown("ws")),
    ("js_ws_handle_upgrade",                        OwnerKind::WellKnown("ws")),
    ("js_ws_send_to_client",                        OwnerKind::WellKnown("ws")),
    ("js_ws_close_client",                          OwnerKind::WellKnown("ws")),
    ("js_ws_server_new",                            OwnerKind::WellKnown("ws")),
    ("js_ws_server_clients",                        OwnerKind::WellKnown("ws")),
    ("js_ws_server_address",                        OwnerKind::WellKnown("ws")),
    ("js_ws_server_emit",                           OwnerKind::WellKnown("ws")),
    ("js_ws_server_close",                          OwnerKind::WellKnown("ws")),
    ("js_ws_server_close_with",                     OwnerKind::WellKnown("ws")),

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

    ("js_ext_net_socket_write3",                    OwnerKind::WellKnown("net")),
    ("js_ext_net_socket_end3",                      OwnerKind::WellKnown("net")),

];

/// Prefix-based routing for ext-binding FFI whose emitted symbols follow the
/// stable `js_<binding>_*` naming convention 1:1 with a `perry-ext-*` wrapper.
///
/// The per-symbol [`FFI_REGISTRY`] above is exhaustive but hand-maintained: it
/// only routes the exact symbols someone remembered to add. That is fine for
/// bindings whose symbol names DON'T map cleanly to the binding key
/// (`js_node_http_*` → `http`), but it silently
/// loses whole families whose symbols DO follow the convention. A program whose
/// codegen lowers to a family symbol that is in NEITHER the exact table nor any
/// import set (e.g. an AOT-compiled `perry.compilePackages` member) would never
/// fire the well-known flip, and the link would die with an undefined
/// `js_<binding>_*` symbol even though the `perry-ext-*` wrapper exists.
///
/// This prefix net closes that gap generally: ANY emitted `js_<binding>_*`
/// symbol for a listed ext binding flips its well-known wrapper onto the link
/// line off codegen provenance alone — no source-level import, no
/// `compilePackages` exclusion, no membership in the well-known iteration set
/// required. Adding a symbol to one of these families needs no table edit.
///
/// The routing key is the exact `well_known_bindings.toml` binding key, so the
/// driver's existing routing (`build_optimized_libs`) resolves the crate/lib and
/// strips the duplicate perry-stdlib feature with no further wiring. Prefixes
/// are checked only AFTER the exact table and the crypto net, so a symbol that
/// wants bespoke routing can always override its family by adding an exact row.
#[rustfmt::skip]
const EXT_PREFIX_REGISTRY: &[(&str, &str)] = &[
    // @parcel/watcher's notify-backed native-addon facade.
    ("js_parcel_watcher_", "@parcel/watcher"),
    // undici HTTP/1.1 client + Agent/ProxyAgent (perry-ext-undici).
    ("js_undici_",     "undici"),
    // Native runtime TypeScript transpilation subset (#8511).
    ("js_typescript_", "typescript"),
    // Bun runtime transpilation/build subset shares the pinned SWC wrapper.
    ("js_bun_transpiler_", "typescript"),
    ("js_bun_build", "typescript"),
];

/// Return the well-known binding that provides an emitted FFI symbol. The
/// compile driver's pre-link check uses the same exact and prefix registries
/// as codegen's provider routing, including the two-provider Bun server call.
pub fn well_known_owner_for_symbol(symbol: &str) -> Option<&'static str> {
    if symbol == "js_bun_serve" {
        return Some("http");
    }
    if let Some((_, owner)) = FFI_REGISTRY.iter().find(|(name, _)| *name == symbol) {
        return match owner {
            OwnerKind::WellKnown(key) => Some(key),
            OwnerKind::Stdlib { .. } => None,
        };
    }
    EXT_PREFIX_REGISTRY
        .iter()
        .find(|(prefix, _)| symbol.starts_with(prefix))
        .map(|(_, binding)| *binding)
}

/// The well-known binding key `module` names when the registry files wrapper
/// symbols under it ([`wrapper_owns_symbols`]).
pub fn wrapper_binding(module: &str) -> Option<&'static str> {
    FFI_REGISTRY
        .iter()
        .find_map(|(_, owner)| match owner {
            OwnerKind::WellKnown(key) if *key == module => Some(*key),
            _ => None,
        })
        .or_else(|| {
            EXT_PREFIX_REGISTRY
                .iter()
                .find(|(_, binding)| *binding == module)
                .map(|(_, binding)| *binding)
        })
}

/// Does this registry name any symbol that only `module`'s wrapper crate
/// defines (the same attribution [`well_known_owner_for_symbol`] makes)?
/// Symbols the stdlib also implements are `OwnerKind::Stdlib`, not listed
/// under the wrapper.
pub fn wrapper_owns_symbols(module: &str) -> bool {
    FFI_REGISTRY
        .iter()
        .any(|(_, owner)| matches!(owner, OwnerKind::WellKnown(key) if *key == module))
        || EXT_PREFIX_REGISTRY
            .iter()
            .any(|(_, binding)| *binding == module)
}

/// The namespace install hook a wrapper crate defines, by its staticlib stem
/// (`perry_ext_http` → `js_ext_http_nm_install`), when this registry lists one.
/// A hook registers the wrapper's value-form export dispatcher and then chains
/// to the runtime bucket install; `crate::native_routing` emits it for modules
/// the compile routes to that wrapper.
pub fn wrapper_install_hook(lib: &str) -> Option<&'static str> {
    let stem = lib.strip_prefix("perry_")?;
    FFI_REGISTRY.iter().find_map(|(name, owner)| {
        (matches!(owner, OwnerKind::WellKnown(_))
            && name
                .strip_prefix("js_")
                .and_then(|rest| rest.strip_suffix("_nm_install"))
                == Some(stem))
        .then_some(*name)
    })
}

/// Every wrapper install hook this registry lists, for declaration.
pub(crate) fn wrapper_install_hooks() -> impl Iterator<Item = &'static str> {
    FFI_REGISTRY.iter().filter_map(|(name, owner)| {
        (matches!(owner, OwnerKind::WellKnown(_))
            && name.starts_with("js_ext_")
            && name.ends_with("_nm_install"))
        .then_some(*name)
    })
}

/// The providers one registry symbol needs, and the marker the per-module
/// capture records for it: the exact symbol for a table row, the matched
/// prefix for a prefix-net symbol. A marker resolves to the same owners as
/// the symbol it stands for, so a manifest stored beside a cached `.o` is
/// checked against today's table, never a routing decision baked in by an
/// older perry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SymbolOwners {
    pub marker: &'static str,
    owners: [Option<OwnerKind>; 2],
}

impl SymbolOwners {
    fn one(marker: &'static str, owner: OwnerKind) -> Self {
        SymbolOwners {
            marker,
            owners: [Some(owner), None],
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = OwnerKind> + '_ {
        self.owners.iter().flatten().copied()
    }
}

/// Which providers `symbol` needs, or `None` for a symbol the always-linked
/// runtime serves. O(N) over `FFI_REGISTRY` (N ≈ 350), only on emission.
pub fn symbol_owners(symbol: &str) -> Option<SymbolOwners> {
    // #9603: `js_bun_serve` is implemented by perry-ext-http but crosses a
    // narrow JSON bridge into perry-stdlib's Fetch registries to construct
    // Request values and consume Response values. It therefore has two
    // providers, unlike the ordinary one-symbol/one-owner registry rows.
    if symbol == "js_bun_serve" {
        return Some(SymbolOwners {
            marker: "js_bun_serve",
            owners: [
                Some(OwnerKind::WellKnown("http")),
                Some(OwnerKind::Stdlib {
                    feature: Some("web-fetch"),
                }),
            ],
        });
    }
    if let Some((name, owner)) = FFI_REGISTRY.iter().find(|(name, _)| *name == symbol) {
        return Some(SymbolOwners::one(name, *owner));
    }
    // Prefix safety net (stdlib cherry-pick): the auto-optimize driver no
    // longer force-enables perry-stdlib's `crypto` feature for every build,
    // so ANY codegen-emitted call into the node:crypto / WebCrypto surface
    // needs the feature: every FFI defined by perry-stdlib's crypto/webcrypto
    // modules carries one of these two prefixes. `js_webcrypto_illegal_constructor`
    // is excluded — it lives in perry-runtime (object/global_this/ctor_thunks.rs).
    // The marker is the prefix itself, which re-enters this arm.
    if symbol.starts_with("js_crypto_")
        || (symbol.starts_with("js_webcrypto_") && symbol != "js_webcrypto_illegal_constructor")
    {
        return Some(SymbolOwners::one(
            "js_crypto_",
            OwnerKind::Stdlib {
                feature: Some("crypto"),
            },
        ));
    }
    // Ext-binding prefix net: an emitted `js_<binding>_*` symbol needs its
    // well-known wrapper (an AOT-compiled `undici` / `typescript` is in no
    // import set). The marker is the matched prefix.
    EXT_PREFIX_REGISTRY
        .iter()
        .find(|(prefix, _)| symbol.starts_with(prefix))
        .map(|(prefix, binding)| SymbolOwners::one(prefix, OwnerKind::WellKnown(binding)))
}

/// The providers a compilation links, computed from HIR before codegen
/// ([`crate::providers::program_providers`]). Codegen does not add to it: an
/// emitted registry symbol whose owner is missing is a compile error
/// ([`check_emitted_providers`]).
pub type ProviderSet = std::collections::BTreeSet<OwnerKind>;

/// Check one module's emitted registry symbols (or the markers a cached
/// object's manifest recorded) against the precomputed provider set. A miss
/// means the HIR provider pass did not predict a provider this module's code
/// calls into: linking would fail with an undefined symbol, or link a runtime
/// the GC effects table does not describe. It is a hard error naming the
/// symbol and its owner.
pub fn check_emitted_providers<'a>(
    providers: &ProviderSet,
    module: &str,
    symbols: impl IntoIterator<Item = &'a str>,
) -> Result<(), String> {
    for symbol in symbols {
        let Some(owners) = symbol_owners(symbol) else {
            continue;
        };
        for owner in owners.iter() {
            if !providers.contains(&owner) {
                return Err(format!(
                    "internal error: module '{module}' emits `{symbol}`, whose provider {owner:?} \
                     is not in the link set computed from HIR before codegen \
                     (perry_codegen::providers). The HIR provider pass must predict every \
                     provider codegen calls into."
                ));
            }
        }
    }
    Ok(())
}

// Per-module capture buffer, active only between [`begin_module_capture`]
// and [`take_module_capture`] on the same thread.
//
// The compile driver checks each module's capture against the provider set
// computed from HIR, and the object cache persists it next to the module's
// cached `.o` so a hit is checked the same way. `perry-codegen` itself never
// uses rayon and `compile_module` runs start to finish on its caller's worker
// thread, so a thread-local scoped around that one call captures exactly
// this module's emissions. Markers, not owners, are recorded (see
// [`SymbolOwners`]).
thread_local! {
    static MODULE_CAPTURE: RefCell<Option<HashSet<&'static str>>> = const { RefCell::new(None) };
}

/// Called from every `LlBlock::call` / `LlBlock::call_void` site: records a
/// registry symbol in this module's capture, which the driver checks against
/// the precomputed provider set ([`check_emitted_providers`]). It never adds
/// a provider.
pub(crate) fn record_ffi_call(symbol: &str) {
    if let Some(owners) = symbol_owners(symbol) {
        MODULE_CAPTURE.with(|cell| {
            if let Some(set) = cell.borrow_mut().as_mut() {
                set.insert(owners.marker);
            }
        });
    }
}

/// Start capturing this thread's registry-symbol emissions. Call
/// immediately before `compile_module`; pair with [`take_module_capture`].
///
/// Idempotent by design: an already-active capture is reset, so a panic
/// that skipped a `take_module_capture` can't leak one module's symbols
/// into the next module compiled on the same rayon worker.
pub fn begin_module_capture() {
    MODULE_CAPTURE.with(|cell| {
        *cell.borrow_mut() = Some(HashSet::new());
    });
}

/// End the capture started by [`begin_module_capture`] and return the
/// registry markers emitted since, sorted so the persisted manifest is
/// byte-identical across runs (the cached `.o` already is — #686).
///
/// Returns an empty vec when no capture was active.
pub fn take_module_capture() -> Vec<&'static str> {
    MODULE_CAPTURE.with(|cell| {
        let mut syms: Vec<&'static str> = cell
            .borrow_mut()
            .take()
            .map(|set| set.into_iter().collect())
            .unwrap_or_default();
        syms.sort_unstable();
        syms
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    // Test shims: the tests below were written against a process-wide
    // collector. Owners now come from `symbol_owners`; these thread-local
    // helpers keep each test's "emit, then ask which providers" shape
    // without any global state (so no cross-test lock is needed).
    thread_local! {
        static RECORDED: RefCell<HashSet<OwnerKind>> = RefCell::new(HashSet::new());
    }

    fn record_ffi_call(symbol: &str) {
        super::record_ffi_call(symbol);
        if let Some(owners) = symbol_owners(symbol) {
            RECORDED.with(|r| r.borrow_mut().extend(owners.iter()));
        }
    }

    fn take_used_providers() -> HashSet<OwnerKind> {
        RECORDED.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }

    /// A cached manifest's markers resolve like the live symbols.
    fn replay_ffi_symbols<I, S>(symbols: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for symbol in symbols {
            record_ffi_call(symbol.as_ref());
        }
    }

    struct ProviderTestGuard;

    impl ProviderTestGuard {
        fn new() -> Self {
            ProviderTestGuard
        }
    }

    #[test]
    fn check_emitted_providers_names_the_missing_owner() {
        let mut set = ProviderSet::new();
        set.insert(OwnerKind::WellKnown("ws"));
        assert!(check_emitted_providers(&set, "m.ts", ["js_ws_send", "js_array_push_f64"]).is_ok());
        let err = check_emitted_providers(&set, "m.ts", ["js_crypto_"]).unwrap_err();
        assert!(
            err.contains("`js_crypto_`") && err.contains("crypto"),
            "{err}"
        );
        // js_bun_serve needs both of its providers.
        set.insert(OwnerKind::WellKnown("http"));
        assert!(check_emitted_providers(&set, "m.ts", ["js_bun_serve"]).is_err());
    }

    /// #6439: per-module capture must see exactly what this thread emitted,
    /// sorted and deduped, and must end cleanly. This is what the object
    /// cache persists beside each `.o`.
    ///
    #[test]
    fn module_capture_collects_only_registered_symbols_sorted() {
        begin_module_capture();
        record_ffi_call("js_ws_send");
        record_ffi_call("js_ws_connect_start");
        record_ffi_call("js_ws_send"); // duplicate: deduped
        record_ffi_call("js_definitely_not_registered_anywhere");
        let captured = take_module_capture();
        assert_eq!(captured, vec!["js_ws_connect_start", "js_ws_send"]);
        // Capture is over: later emissions belong to the next module.
        record_ffi_call("js_ws_close");
        assert!(
            take_module_capture().is_empty(),
            "take_module_capture must end the capture"
        );
    }

    /// A fresh capture must not inherit the previous one's symbols — rayon
    /// reuses worker threads, so module N+1 would otherwise be credited with
    /// module N's FFI and drag unused provider crates onto the link line.
    #[test]
    fn begin_module_capture_resets_previous_capture() {
        begin_module_capture();
        record_ffi_call("js_ws_send");
        begin_module_capture(); // no take_module_capture in between
        record_ffi_call("js_ws_close");
        assert_eq!(take_module_capture(), vec!["js_ws_close"]);
    }

    /// Replay must route a manifest's symbols exactly as live codegen would
    /// — this is the whole point: a cache hit and a cache miss must produce
    /// the same providers, hence the same link line.
    #[test]
    fn replayed_symbols_route_like_live_codegen() {
        let _guard = ProviderTestGuard::new();
        let _ = take_used_providers();
        replay_ffi_symbols(["js_ws_connect_start", "js_not_a_real_symbol"]);
        let got = take_used_providers();
        assert!(
            got.contains(&OwnerKind::WellKnown("ws")),
            "replayed ws symbol must record the ws provider, got {:?}",
            got
        );
        assert_eq!(got.len(), 1, "unknown symbols must not add providers");
    }

    fn assert_symbol_routes_to(symbol: &str, owner: OwnerKind) {
        let _ = take_used_providers();
        record_ffi_call(symbol);
        let got = take_used_providers();
        assert!(
            got.contains(&owner),
            "{} must route to {:?}, got {:?}",
            symbol,
            owner,
            got
        );
    }

    #[test]
    fn registry_dispatch_routes_to_correct_owner() {
        let _guard = ProviderTestGuard::new();
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
        // Repro #9604: one stdlib-backed export must not make every `bun`
        // import pull in SQLite; the emitted constructor is the feature gate.
        record_ffi_call("js_bun_sql_new");
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
            got.contains(&OwnerKind::Stdlib {
                feature: Some("database-sqlite")
            }),
            "expected Stdlib(database-sqlite) for Bun.SQL, got {:?}",
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
        let _guard = ProviderTestGuard::new();
        assert_symbol_routes_to(
            "js_node_http_create_server_with_options",
            OwnerKind::WellKnown("http"),
        );
    }

    /// #10428/#10429: materializing a `net` / `http`/`https`/`http2`
    /// namespace (or a bound export such as `require('net').createConnection`)
    /// emits the PROVIDER's install wrapper, which registers the value-form
    /// dispatcher. It lives in the ext crate, so emitting it must flip that
    /// crate onto the link line even when no import made it visible.
    #[test]
    fn provider_namespace_installs_route_to_their_well_known_binding() {
        let _guard = ProviderTestGuard::new();
        use crate::native_routing::{NativeProvider, NativeRouting};
        let wrapper = |lib: &str| NativeProvider::Wrapper(lib.to_string());
        let routed = NativeRouting::new([
            ("net".to_string(), wrapper("perry_ext_net")),
            ("http".to_string(), wrapper("perry_ext_http")),
            ("https".to_string(), wrapper("perry_ext_http")),
            ("http2".to_string(), wrapper("perry_ext_http")),
        ]);
        for (module, owner) in [
            ("net", "net"),
            ("node:net", "net"),
            ("http", "http"),
            ("node:https", "http"),
            ("http2", "http"),
        ] {
            let symbol = routed
                .install_symbol(module)
                .unwrap_or_else(|| panic!("{module} has no namespace install symbol"));
            assert_symbol_routes_to(symbol, OwnerKind::WellKnown(owner));
        }
        // A module the bundled libraries serve installs the runtime bucket,
        // which asks the linker for no wrapper.
        let bundled = NativeRouting::new([(
            "http".to_string(),
            NativeProvider::Bundled("perry_ext_http".to_string()),
        )]);
        let symbol = bundled.install_symbol("http").expect("http bucket");
        let _ = take_used_providers();
        record_ffi_call(symbol);
        assert!(
            take_used_providers().is_empty(),
            "{symbol} must record no provider"
        );
    }

    #[test]
    fn bun_serve_routes_to_http_and_fetch_providers() {
        let _guard = ProviderTestGuard::new();
        let _ = take_used_providers();
        record_ffi_call("js_bun_serve");
        let got = take_used_providers();
        assert!(got.contains(&OwnerKind::WellKnown("http")));
        assert!(got.contains(&OwnerKind::Stdlib {
            feature: Some("web-fetch")
        }));
    }

    /// #3954 regression: HTTP-suite native-table rows can emit newer
    /// `perry-ext-http` or `perry-ext-net`
    /// symbols without the module-import path being visible to collection.
    /// Each emitted external symbol must independently flip its well-known
    /// owner so the wrapper joins the link line.
    #[test]
    fn emitted_http_suite_external_symbols_route_to_owners() {
        let _guard = ProviderTestGuard::new();
        for symbol in [
            "js_http_request_overload",
            "js_http_set_header",
            "js_http_client_request_get_header",
            "js_http_client_request_destroyed",
            "js_https_get_overload",
            "js_node_http_im_set_timeout",
            "js_node_http_res_cork",
            "js_node_http_res_append_header",
            "js_node_http_res_set_header_self",
            "js_node_http_res_has_header_value",
            "js_node_http_res_write_early_hints",
            "js_node_http_res_set_send_date",
            "js_node_http_res_set_strict_content_length",
            "js_net_socket_listeners",
            "js_net_socket_raw_listeners",
            "js_ws_on_client_i64",
            "js_ws_send_client_i64",
            "js_ws_close_client_i64",
        ] {
            let owner = match symbol {
                symbol if symbol.starts_with("js_net_") => OwnerKind::WellKnown("net"),
                symbol if symbol.starts_with("js_ws_") => OwnerKind::WellKnown("ws"),
                _ => OwnerKind::WellKnown("http"),
            };
            assert_symbol_routes_to(symbol, owner);
        }
    }

    /// Stdlib cherry-pick: the driver no longer force-enables perry-stdlib's
    /// `crypto` feature, so every codegen-emitted `js_crypto_*` /
    /// `js_webcrypto_*` call must flip it through the prefix net — including
    /// the `"js_crypto_"` marker persisted to (and replayed from) the object
    /// cache's FFI manifest. `js_webcrypto_illegal_constructor` lives in
    /// perry-runtime and must NOT flip the feature.
    #[test]
    fn emitted_crypto_symbols_route_to_stdlib_crypto_feature() {
        let _guard = ProviderTestGuard::new();
        let crypto_owner = OwnerKind::Stdlib {
            feature: Some("crypto"),
        };
        for symbol in [
            "js_crypto_sha256",
            "js_crypto_create_hash",
            "js_crypto_ed25519_verify",
            "js_webcrypto_digest",
            // Object-cache replay marker (see record_ffi_call).
            "js_crypto_",
        ] {
            assert_symbol_routes_to(symbol, crypto_owner);
        }

        let _ = take_used_providers();
        record_ffi_call("js_webcrypto_illegal_constructor");
        let got = take_used_providers();
        assert!(
            !got.contains(&crypto_owner),
            "js_webcrypto_illegal_constructor is a perry-runtime thunk and must \
             not flip the stdlib crypto feature, got {got:?}"
        );
    }

    /// The prefix net must persist a replayable marker in the per-module
    /// capture so a warm object cache reproduces the same feature flip
    /// (#6439 shape).
    #[test]
    fn crypto_prefix_net_marker_survives_module_capture_replay() {
        let _guard = ProviderTestGuard::new();
        let _ = take_used_providers();

        begin_module_capture();
        record_ffi_call("js_crypto_pbkdf2");
        let captured = take_module_capture();
        assert_eq!(captured, vec!["js_crypto_"]);

        let _ = take_used_providers();
        replay_ffi_symbols(captured);
        let got = take_used_providers();
        assert!(
            got.contains(&OwnerKind::Stdlib {
                feature: Some("crypto")
            }),
            "replaying the captured marker must reproduce the crypto flip, got {got:?}"
        );
    }

    /// #2013 regression: net validation fixtures emit provider-only
    /// `perry-ext-net` symbols through native-table lowering. Each one must
    /// route to the net well-known archive so `PERRY_NO_AUTO_OPTIMIZE=1`
    /// compiles the fixtures instead of linking unresolved `js_net_*` calls.
    #[test]
    fn emitted_net_validation_external_symbols_route_to_net() {
        let _guard = ProviderTestGuard::new();
        for symbol in [
            "js_net_create_server",
            "js_net_server_listen",
            "js_net_get_default_auto_select_family",
            "js_net_set_default_auto_select_family",
            "js_net_get_default_auto_select_family_attempt_timeout",
            "js_net_set_default_auto_select_family_attempt_timeout",
            "js_net_socket_set_timeout",
            "js_net_socket_get_type_of_service",
            "js_net_socket_set_type_of_service",
        ] {
            assert_symbol_routes_to(symbol, OwnerKind::WellKnown("net"));
        }
    }

    /// tokio lane L4: perry-stdlib's bundled `net` copy — the other definition
    /// of the socket core and `tls.connect` — is deleted, so every one of these
    /// must pull libperry_ext_net.a onto the link line when emitted.
    #[test]
    fn emitted_net_socket_core_symbols_route_to_net() {
        let _guard = ProviderTestGuard::new();
        for symbol in [
            "js_net_socket_alloc",
            "js_net_socket_connect",
            "js_net_socket_method_connect",
            "js_net_socket_on",
            "js_net_socket_read",
            "js_net_socket_write",
            "js_net_socket_end",
            "js_net_socket_destroy",
            "js_net_socket_upgrade_tls",
            "js_tls_connect",
        ] {
            assert_symbol_routes_to(symbol, OwnerKind::WellKnown("net"));
        }
    }

    /// The prefix-family link gap: an AOT-compiled `perry.compilePackages`
    /// member (so never in `native_module_imports` and never in the
    /// well-known iteration set) can lower to a `js_<binding>_*` family
    /// symbol. The prefix net must route each registered ext family
    /// to its well-known wrapper off codegen provenance alone — no exact-table
    /// row per symbol required.
    #[test]
    fn emitted_ext_prefix_symbols_route_to_well_known_binding() {
        let _guard = ProviderTestGuard::new();
        for (symbol, binding) in [
            ("js_undici_request", "undici"),
            ("js_undici_proxy_agent_new", "undici"),
            ("js_parcel_watcher_subscribe", "@parcel/watcher"),
            ("js_parcel_watcher_get_events_since", "@parcel/watcher"),
            ("js_typescript_transpile_module", "typescript"),
            ("js_bun_transpiler_new", "typescript"),
            ("js_bun_build", "typescript"),
        ] {
            assert_symbol_routes_to(symbol, OwnerKind::WellKnown(binding));
        }
    }

    /// The prefix net must NOT swallow symbols that are not part of a listed
    /// ext family: an unknown `js_*` symbol records no provider, and a
    /// prefix-family symbol must never leak into a DIFFERENT binding.
    #[test]
    fn ext_prefix_net_does_not_over_match() {
        let _guard = ProviderTestGuard::new();
        let _ = take_used_providers();
        record_ffi_call("js_undici_request");
        let got = take_used_providers();
        assert!(got.contains(&OwnerKind::WellKnown("undici")));
        assert!(
            !got.contains(&OwnerKind::WellKnown("@parcel/watcher")),
            "undici symbol must not flip @parcel/watcher, got {got:?}"
        );

        let _ = take_used_providers();
        record_ffi_call("js_some_unrelated_runtime_symbol");
        assert!(
            take_used_providers().is_empty(),
            "an unlisted js_* symbol must record no provider"
        );
    }

    /// The prefix-family capture marker (the matched prefix) must survive an
    /// object-cache round trip: replaying it reproduces the same well-known
    /// flip, so a warm `.o` cache links identically to a cold one (#6439 shape).
    #[test]
    fn ext_prefix_marker_survives_module_capture_replay() {
        let _guard = ProviderTestGuard::new();
        let _ = take_used_providers();

        begin_module_capture();
        record_ffi_call("js_undici_request");
        record_ffi_call("js_undici_proxy_agent_new"); // same family: one marker, deduped
        let captured = take_module_capture();
        assert_eq!(captured, vec!["js_undici_"]);

        let _ = take_used_providers();
        replay_ffi_symbols(captured);
        let got = take_used_providers();
        assert!(
            got.contains(&OwnerKind::WellKnown("undici")),
            "replaying the captured prefix marker must reproduce the undici flip, got {got:?}"
        );
    }
}
