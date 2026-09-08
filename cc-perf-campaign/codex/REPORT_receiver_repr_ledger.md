# Receiver-representation campaign PR 1: diagnostics and typed ledger

Date: 2026-09-07 CEST

Branch: `perf/receiver-repr-ledger`

## Revisions

- base `origin/main`: `8b7dc3342b22fe6270739c8d51585c3d2cdfa618`
- implementation: `9b6884e214beb93873f21dd76afe49e42f3f25df`
- report: this final branch commit (the pushed head named in the handoff)

The merge-base is exactly the base SHA above. No #9937 commit or tree was used.

## Result

PR 1 adds measurement and metadata only. It does not change a JS-visible
representation, a release-path classifier, a wrapper, or native-result boxing.
All five new pointer-result classes enter the same `LoweredValue::native_handle`
branch used by the removed `NativeRetKind::Ptr`, so materialization is byte-for-
byte the existing pointer-tag path. `observed_wrapped` is therefore intentionally
zero in this PR.

`PERRY_RECEIVER_REPR_DIAG=<sink>` reuses `hot_diag`'s sink parser and atomic
temporary-file rename. The first event publishes a snapshot; later events test
every 4,096 events and replace the file after at least one second. The line is:

```text
[receiver-repr-diag] constructed common=… fetch=… zlib=… proxy=… timer=… text=… tui=… async_hook=… async_resource=… symbol_global=… external_buffer=… sab=… null_stub=…; observed_old common=… fetch=… zlib=… proxy=… timer=… text=… tui=… async_hook=… async_resource=… symbol_global=… external_buffer=… sab=… null_stub=…; observed_wrapped common=… fetch=… zlib=… proxy=… timer=… text=… tui=… async_hook=… async_resource=… symbol_global=… external_buffer=… sab=… null_stub=…; bare_managed=…; invalid_pointer_zero=…; direct_mismatch=…
```

Every producer and funnel has one `receiver_repr_on()` guard. After one-time
environment parsing, the test is a relaxed atomic load. When unarmed, no range,
registry, ownership, mutex, header, counter, or snapshot work runs. The only
release-path cost is that single guard. `direct_mismatch`'s direct header-byte
probe is compiled only with `debug_assertions`; even there it runs only when the
sink is armed.

## Instrumented producers

The current branch locations are:

| Family | Producer(s) and diagnostic bump |
| --- | --- |
| common | `register_handle` at `crates/perry-stdlib/src/common/handle.rs:43` (bump `:47`); `register_handle_with_id` at `:55` (bump `:58`) |
| fetch | `alloc_fetch_handle_id` at `crates/perry-stdlib/src/fetch/mod.rs:223` (bump `:231`) |
| zlib | `next_zlib_id` at `crates/perry-stdlib/src/zlib.rs:722` (bump `:730`) |
| proxy | `js_proxy_new` at `crates/perry-runtime/src/proxy.rs:451` (bump `:483`) |
| timer | `next_timer_id` at `crates/perry-runtime/src/timer.rs:529` (bump `:534`) |
| text | `js_text_encoder_new` at `crates/perry-runtime/src/text.rs:106` (bump `:108`); decoder `register_decoder` at `:159` (bump `:179`) |
| tui | tree `register` at `crates/perry-runtime/src/tui/tree.rs:51` (bump `:55`); `js_perry_tui_state_alloc` at `tui/state.rs:100` (bump `:105`); `use_ref`, `use_app`, `use_stdout`, and `use_focus_manager` at `tui/hooks.rs:541,602,642,828` (bumps `:555,604,644,830`) |
| async_hook | `js_async_hooks_create_hook` at `crates/perry-runtime/src/async_hooks.rs:558` (bump `:572`) |
| async_resource | `new_async_resource_with_public_value` at `crates/perry-runtime/src/async_hooks.rs:1222` (bump `:1251`) |
| symbol_global | `well_known_symbol` at `crates/perry-runtime/src/symbol.rs:313` (bump `:336`); `intl_legacy_constructed_symbol` at `:642` (bump `:659`); `js_symbol_for` at `symbol/constructors.rs:44` (bump `:111`) |
| external_buffer | `js_buffer_register_external` at `crates/perry-runtime/src/buffer/header.rs:607` (bump `:617`) |
| sab | `alloc_shared_sab` at `crates/perry-runtime/src/shared_sab.rs:60` (bump `:88`) |
| null_stub | `js_unresolved_namespace_stub` at `crates/perry-runtime/src/object/null_stub.rs:37` (bump `:40`) |

The five receiver/property observations are guarded at
`gc_pointer_and_type_from_value` (`object/native_call_method.rs:1025`),
`dispatch_primitive` (`object/native_call_method/primitive_methods.rs:15`),
`object_static_prototype` (`object/prototype_chain.rs:305`), the object field
tail (`object/field_get_set/get_field_by_name_tail.rs:11`), and `ic_miss`
(`object/field_get_set/ic_miss.rs:541`).

Common/fetch/zlib use their audited reserved bands. Proxy, timer, decoder, TUI,
async hook/resource, symbol, external-buffer, SAB, and null-stub classification
uses authoritative registries or exact singleton identity. Because the old
encoding has no provenance and small integer registries overlap, one payload
may increment every semantic family whose registry/range claims it. That is an
intentional measurement of the old representation's ambiguity, not a new
release classifier.

## Typed native-result ledger

The executable declaration census on current `origin/main` is:

| Kind | Rows | Meaning |
| --- | ---: | --- |
| `NR_GCPTR` | 131 | non-null managed Perry allocation with `GcHeader` |
| `NR_NULLABLE_GCPTR` | 2 | managed allocation, with zero retaining the provider's current null/failure behavior |
| `NR_HANDLE_ID` | 221 | integer registry id or sentinel |
| `NR_FOREIGN_PTR` | 4 | headerless native `Box` address (AsyncHook/AsyncResource backing) |
| `NR_JS_VALUE` | 13 | raw NaN-boxed JS bits in the integer ABI slot |
| **total** | **371** | every executable declaration formerly using the erased pointer kind |

The campaign's quoted “372 rows” came from a textual `ret: NR_PTR` census. On
this base that text consists of 370 executable `NativeModSig.ret` fields and
two Fastify prose comments. Conversely, `http_client.rs` has one real result
kind in the positional `cr(...)` helper that the textual census missed.
Therefore the executable total is 371, not 372: 370 fields plus the helper.
Both comments were corrected and the missed helper is explicitly
`NR_HANDLE_ID`. No executable `NR_PTR` remains.

`scripts/native_result_ledger.tsv` records 322 distinct runtime symbols, their
class, provider source, and provider Rust return type. The 81 symbols/rows that
the design's earlier signature census left unresolved were checked against
their provider implementations while constructing this inventory: **81/81
resolved, zero unknown**. The checked provider set spans runtime, stdlib, and
the in-tree `perry-ext-*` implementations (rather than Android stubs or
force-link declarations).

`scripts/native_result_ledger.py` checks both sides: every typed executable row
must agree with one provider ledger entry, every provider source must exist and
declare the symbol, no provider entry may be stale or classless, and a legacy
`NR_PTR` declaration is an immediate failure. A dedicated path-filtered CI
workflow runs `--self-test` and the real inventory.

## Fail-capable tests and sabotage

- `receiver_repr_family_fixtures_move_constructed_and_observed_old` constructs
  real runtime proxy, timer, text, TUI, AsyncHook, AsyncResource, global-symbol,
  external-buffer, SAB, and null-stub values, then sends each through the armed
  receiver classifier. Common/fetch/zlib are dependency-lower producers, so
  the fixture exercises their exact reserved bands and the companion source
  witness binds them to their real producer bumps. Every family asserts
  `constructed > 0`, `observed_old > 0`, and `observed_wrapped == 0`.
- `receiver_repr_every_family_producer_keeps_its_constructed_bump` pins the
  exact number of bump sites in every audited producer file. Sabotage actually
  performed: changed the first common producer's family token to `Fetch` and
  ran the already-built release test. It failed exit 101 with `left: 1,
  right: 2`; restoring the token returned the test to green. Dropping the bump
  has the same failing count.
- `receiver_repr_unarmed_funnels_never_enter_classification` invokes all five
  real funnels with the sink forced off and asserts the test-only classifier
  entry count stays zero. Sabotage: delete any funnel guard; that funnel calls
  `receiver_repr_note_*`, the entry counter becomes nonzero, and the equality
  fails.
- `native_result_ledger.py --self-test` plants a legacy `NR_PTR` row and sees
  the checker reject it, then plants a table/provider class disagreement and
  sees that rejected too. Sabotage: re-add any erased row or remove/change its
  provider class; the CI command exits 1.

## Gates

Each Cargo command used the campaign build lock and had `df -g /` checked
immediately before launch. Available space was 23 GB (runtime release test),
17 GB (codegen test), 17 GB (wasm-host build), and 14 GB (CLI build), all above
the required 12 GB launch floor.

- `cargo test -p perry-runtime --release --lib -j6 -- --test-threads=1`:
  PASS in 8m11s build + 7.02s tests; 3,262 passed, 4 ignored, 0 failed.
- `cargo test -p perry-codegen --lib -j6`: PASS; 1,450 passed, 1 ignored,
  0 failed (latest incremental build 16.49s, tests 2.83s).
- `python3 scripts/native_result_ledger.py --self-test`: PASS; planted erased
  row and provider mismatch both rejected.
- `python3 scripts/native_result_ledger.py`: PASS; 371 rows, 322 providers,
  counts exactly as above.
- `python3 scripts/gc_runtime_root_holders.py`: PASS; 1,363 declarations,
  1,160 identity-ratcheted, 593 scanner-reached, 350 inventory-classified,
  415 frontier-pinned, 152 registered scanners.
- `python3 scripts/check_thread_locals.py`: PASS; 405 hot declarations, 273
  cold declarations in 83 recorded files, capacity 768. No new TLS was added.
- `cargo build --release -p perry-runtime --features wasm-host -j6`: PASS in
  2m32s.
- `cargo build --release -p perry -j6`: PASS in 11m33s. Seven pre-existing
  feature-dependent dead-code warnings were emitted from GC barrier/regexp
  code; none names a changed diagnostic or ledger item.
- direct `rustfmt` over every touched Rust file: PASS.
- `git diff --check`: PASS.
- `scripts/check_file_size.sh`: PASS, “no Rust source files exceed 2000
  lines.” The touched field tail is 1,997 lines.

## Perrymaster measurement request

Relink the best cc bundle against implementation
`9b6884e214beb93873f21dd76afe49e42f3f25df`'s runtime. Run exactly one 3,300
reply with `PERRY_RECEIVER_REPR_DIAG=stderr`, and report the final complete
`[receiver-repr-diag]` line together with the bundle SHA/build identity. Do not
average or omit zero buckets.

That single line is the PR 2 pricing input: `constructed` gives wrapper
creations per reply by family, `observed_old` shows which families reach the
dynamic receiver/property paths, and the zero/nonzero relationship settles
the design's open question 1 for the best cc workload. It also provides the
required baselines for `observed_wrapped`, `bare_managed`,
`invalid_pointer_zero`, and debug-only `direct_mismatch` before any
representation change.
