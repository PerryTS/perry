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
    ("js_net_socket_listeners",                     OwnerKind::WellKnown("net")),
    ("js_net_socket_raw_listeners",                 OwnerKind::WellKnown("net")),
    ("js_net_socket_reset_and_destroy",             OwnerKind::WellKnown("net")),
    ("js_net_server_once",                          OwnerKind::WellKnown("net")),
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

    // ── #5140: EventEmitter without a source-level `import "events"` ──
    // `new EventEmitter()` / `.on` / `.emit` / … lower to these
    // `js_event_emitter_*` helpers purely off the class NAME being
    // `EventEmitter` (`lower_call/builtin.rs` + the events native-table
    // rows), regardless of which package the binding came from. The
    // canonical implementations live in `perry-ext-events` (the
    // `[bindings.events]` well-known crate); `import "events"` flips that
    // wrapper onto the link line via `ctx.native_module_imports`. But a
    // program that gets `EventEmitter` from a *different* package — e.g.
    // `import EventEmitter from "eventemitter3"` under
    // `perry.compilePackages` — never inserts `"events"` into the import
    // set, so the wrapper stays off the link line and the build fails with
    // `Undefined symbols: _js_event_emitter_new_with_options` (and the
    // `_on` / `_emit` / `_remove_all_listeners` companions). Tagging the
    // emitted symbols here makes the well-known flip fire off codegen
    // provenance instead of imports — same mechanism as the #846 / #3954
    // http/net rows above. Flipping `"events"` also activates perry-stdlib's
    // `external-events-construct` feature (see optimized_libs.rs), which the
    // default-import dynamic-`new` path relies on (#4995).
    //
    // Only the core surface defined by perry-ext-events is listed; the
    // `js_event_emitter_async_resource_*` helpers live in perry-stdlib and
    // are out of scope here (`EventEmitterAsyncResource` is node:events-only).
    ("js_event_emitter_new",                        OwnerKind::WellKnown("events")),
    ("js_event_emitter_new_with_options",           OwnerKind::WellKnown("events")),
    ("js_event_emitter_on",                         OwnerKind::WellKnown("events")),
    ("js_event_emitter_once",                       OwnerKind::WellKnown("events")),
    ("js_event_emitter_prepend_listener",           OwnerKind::WellKnown("events")),
    ("js_event_emitter_prepend_once_listener",      OwnerKind::WellKnown("events")),
    ("js_event_emitter_emit",                       OwnerKind::WellKnown("events")),
    ("js_event_emitter_emit0",                      OwnerKind::WellKnown("events")),
    ("js_event_emitter_remove_listener",            OwnerKind::WellKnown("events")),
    ("js_event_emitter_remove_all_listeners",       OwnerKind::WellKnown("events")),
    ("js_event_emitter_listener_count",             OwnerKind::WellKnown("events")),
    ("js_event_emitter_listeners",                  OwnerKind::WellKnown("events")),
    ("js_event_emitter_raw_listeners",              OwnerKind::WellKnown("events")),
    ("js_event_emitter_event_names",                OwnerKind::WellKnown("events")),
    ("js_event_emitter_set_max_listeners",          OwnerKind::WellKnown("events")),
    ("js_event_emitter_get_max_listeners",          OwnerKind::WellKnown("events")),
    ("js_event_emitter_domain_value",               OwnerKind::WellKnown("events")),

    // ── mysql2 (perry-ext-mysql2) ────────────────────────────────────
    // Normally `import "mysql2"` flips the `[bindings.mysql2]` well-known
    // and links perry-ext-mysql2. But a bundler (webpack/turbopack) inlines
    // mysql2 under a NUMERIC module id, so there is no bare import for perry
    // to see — and JS mysql2 JIT-compiles its row parsers with `new Function`
    // (via `generate-function`), which an AOT binary cannot execute. The HIR
    // pass in `perry-hir`'s native-module lowering recognizes a bundled
    // `createPool`/`createConnection` by its mysql2 config-object signature
    // and emits these FFIs directly, WITHOUT adding "mysql2" to the import
    // set. Tag them here so the well-known flip fires off codegen provenance
    // — same mechanism as the http/net/events rows above — and the staticlib
    // joins the link line instead of leaving `_js_mysql2_*` undefined.
    ("js_mysql2_create_pool",                       OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_create_connection",                 OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_query",                        OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_execute",                      OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_get_connection",               OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_end",                          OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_connection_query",             OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_connection_execute",           OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_pool_connection_release",           OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_connection_query",                  OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_connection_execute",                OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_connection_begin_transaction",      OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_connection_commit",                 OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_connection_rollback",               OwnerKind::WellKnown("mysql2")),
    ("js_mysql2_connection_end",                    OwnerKind::WellKnown("mysql2")),
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
        assert_symbol_routes_to(
            "js_node_http_create_server_with_options",
            OwnerKind::WellKnown("http"),
        );
    }

    /// #3954 regression: HTTP-suite native-table rows can emit newer
    /// `perry-ext-http`, `perry-ext-http-server`, or `perry-ext-net`
    /// symbols without the module-import path being visible to collection.
    /// Each emitted external symbol must independently flip its well-known
    /// owner so the wrapper joins the link line.
    #[test]
    fn emitted_http_suite_external_symbols_route_to_owners() {
        let _guard = PROVIDER_TEST_LOCK
            .lock()
            .expect("provider test lock poisoned");
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

    /// #5140 regression: `new EventEmitter()` / `.on` / `.emit` /
    /// `.removeAllListeners` lower to `js_event_emitter_*` helpers off the
    /// class name alone, so a program that imports `EventEmitter` from a
    /// non-`events` package (`import EventEmitter from "eventemitter3"`)
    /// emits these symbols without ever inserting `"events"` into the import
    /// set. Each must route to `WellKnown("events")` so the well-known flip
    /// pulls `perry-ext-events` onto the link line; before the fix the build
    /// failed with `Undefined symbols: _js_event_emitter_new_with_options`.
    #[test]
    fn emitted_event_emitter_symbols_route_to_events() {
        let _guard = PROVIDER_TEST_LOCK
            .lock()
            .expect("provider test lock poisoned");
        for symbol in [
            "js_event_emitter_new",
            "js_event_emitter_new_with_options",
            "js_event_emitter_on",
            "js_event_emitter_once",
            "js_event_emitter_prepend_listener",
            "js_event_emitter_prepend_once_listener",
            "js_event_emitter_emit",
            "js_event_emitter_emit0",
            "js_event_emitter_remove_listener",
            "js_event_emitter_remove_all_listeners",
            "js_event_emitter_listener_count",
            "js_event_emitter_listeners",
            "js_event_emitter_raw_listeners",
            "js_event_emitter_event_names",
            "js_event_emitter_set_max_listeners",
            "js_event_emitter_get_max_listeners",
            "js_event_emitter_domain_value",
        ] {
            assert_symbol_routes_to(symbol, OwnerKind::WellKnown("events"));
        }
    }

    /// #2013 regression: net validation fixtures emit provider-only
    /// `perry-ext-net` symbols through native-table lowering. Each one must
    /// route to the net well-known archive so `PERRY_NO_AUTO_OPTIMIZE=1`
    /// compiles the fixtures instead of linking unresolved `js_net_*` calls.
    #[test]
    fn emitted_net_validation_external_symbols_route_to_net() {
        let _guard = PROVIDER_TEST_LOCK
            .lock()
            .expect("provider test lock poisoned");
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
}
