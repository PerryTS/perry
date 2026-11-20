### Windows: precise GC roots through `try`, and libuv-shaped errnos (#10385)

Extracted from PR #10403, which targets the draft `turnloop/integration`
branch (375 commits off `main`) and so cannot land as it stands. This is the
subset of that work that applies to `main`; the turnloop-only half is left in
#10403. Extraction and validation were done on macOS/aarch64 — **no Windows
box was available, so every Windows-side behavioural claim below is #10403's,
not something re-verified here.** What *was* verified on the host is that the
subset applies to `main`, builds, and leaves non-Windows behaviour unchanged.

#### windows-msvc stops using funclet EH (#7354)

#7302 moved `try`/`catch` to `invoke`/`landingpad` because `longjmp` could skip
a statepoint relocation write-back (#7174). On windows-msvc that lowering
became SEH funclets, and LLVM's `rewrite-statepoints-for-gc` does not support
funclet EH — it crashes on `catchswitch`/`catchpad` (reported still
reproducible on LLVM 22.1.8, not just the 22.1.3 the refusal cites). Windows
could therefore have precise moving-GC roots *or* `try`, never both; since
essentially all async lowers to a `try`, that meant no precise roots in
practice.

windows-msvc now emits the same single landing-pad shape as ELF/Mach-O, with
Perry's own personality. Because the personality is not a recognized MSVC one,
LLVM classifies the function as non-funclet EH and emits
`.seh_handler perry_eh_personality, @unwind, @except` plus an Itanium-format
`GCC_except_table` on COFF — the same LSDA the ELF/Mach-O path already parses.
A new x64 language handler in `eh_windows.rs` walks that table and transfers
control with `RtlUnwindEx`.

- `stmt/try_stmt.rs`: `emit_eh_dispatch` loses its `target_triple.contains("-windows-")`
  branch and always emits `landingpad { ptr, i32 } catch ptr null` under
  `perry_eh_personality`. **For every non-Windows target triple the emitted IR
  is unchanged** — that branch already selected this arm.
- `runtime_decls/strings_part2.rs`: unconditional `declare_personality()`, same
  reason.
- `module.rs`: `declare_seh_machinery()` and `needs_eh_funclets()` deleted.
  Nothing outside the Windows path called either.
- `codegen/mod.rs`: `try_native_construction` no longer declines to the textual
  path for funclet modules — the predicate it consulted was Windows-only.
- `function.rs`: doc comment only.
- `eh.rs` → new `eh_lsda.rs`: the GCC-format LSDA decoder is moved out verbatim
  (byte-identical modulo comments) so both personalities share one decoder.
- `eh_windows.rs`: the SEH filter is replaced by a real x64 language handler.
  `#[cfg(windows)]`-gated; not compiled on any other host. `DispatcherContext`
  is made `pub` — it appears in `pub fn perry_eh_personality`'s signature, and
  the `private_interfaces` warning that provoked is a hard error under
  `-D warnings`.

Two edits beyond #10403's diff were needed, because `eh.rs` is
`#[cfg(not(windows))]` and so was never compiled by the Windows-only
validation behind that PR — as extracted, it did not build on this host:

- `eh_walker.rs` (also `cfg(not(windows))`) calls the decoder through
  `crate::eh::find_landing_pad_in_lsda`, which the move made private. It now
  names `crate::eh_lsda::` directly. This caller does not exist on #10403's
  base.
- `eh.rs`'s LSDA unit tests reach into `DwarfReader` and the `DW_EH_PE_*`
  constants. Rather than widen those back to `pub(crate)`, the test module
  moved to `eh_lsda.rs` alongside the code it exercises — including
  `action_zero_pad_is_still_a_handler`, which pins the #8082 invariant. Its
  `not(target_os = "windows")` guard was belt-and-braces (`eh.rs` is already
  `cfg(not(windows))`), so the tests now run on every host.

Open risk, not resolved here: `DispatcherContext` is documented as the x64
`DISPATCHER_CONTEXT` and carries `offset_of!` asserts, but it is not
`cfg(target_arch)`-guarded, and `windows-arm64-build` is a live CI job. The
fields the handler reads sit at the same offsets in the ARM64 layout, and both
targets are LP64 so the asserts pass either way — which means a real ARM64
divergence would be silent rather than caught. Only an ARM64 Windows run can
settle it; `aarch64-pc-windows-msvc` is not installed on the extraction host.

`linker.rs`'s `rs4gc_funclet_refusal` and `eh_mode.rs`'s module doc still
describe the funclet shape. The refusal is now unreachable defensively rather
than wrong, so both are left as a follow-up rather than churned here.

#### `err.errno` is libuv's number, not the negated OS one

`perry-ext-http`'s `fallback_errno` was a macOS-vs-else pair, which handed
Windows the Linux values; and the concrete-error path negated the raw OS errno,
which is right on Linux/macOS and never right on Windows, where libuv uses its
own `-4xxx` space. A new `libuv_errno(code, raw)` makes that explicit, and a
`#[cfg(windows)]` table supplies the seven network codes. **Non-Windows
behaviour is byte-identical** — the `#[cfg(not(windows))]` arm is the previous
body, unmoved. `ETIMEDOUT = -4039` was cross-checked against
`util_syserr.rs`'s `UV_WINDOWS_ERRNOS`, which `main` already carries and
unit-tests on every host; the other six are outside that table's filesystem
scope and were not independently checkable here.

#### A test that asked the platform instead of the mechanism

`gc/tests/telemetry_verifier.rs` keyed the malloc-maintenance expectation on
`target_env = "gnu" || target_os = "macos"`. `cycle_malloc_trim.rs` also reports
`executed` on any other target when the mimalloc purge ran, and `alloc-mimalloc`
is in perry-runtime's `default`, so the assertion was wrong wherever that is the
live path. It now consults `test_mimalloc_purge_count()` as well. On macOS and
glibc the first arm still short-circuits, so the assertion is unchanged there.

#### `scripts/node_compat_matrix.mjs`

Three Windows breaks, all in the harness rather than the compiler: `PERRY_BIN`
is now honoured (and defaults to `perry.exe` on win32); a probe binary produced
as `out.exe` is found and run rather than reported UNRESOLVED; and the compile
timeout goes 300s → 900s, because a cold ext-routed auto-optimize rebuild
outran it for the *unprefixed* probe only, which the harness then reports as a
prefix divergence — #10403 measured ten fabricated ones cold against four
genuine ones warm.

#### Deliberately not carried from #10403

- The turnloop-only half: `turnloop_net/{abi,errors}.rs`, `event_pump/agent_loop.rs`
  (neither path exists on `main`) and `child_process/reactor.rs`'s `CpPipe`
  migration.
- The `lru-subclass` Cargo feature and the `perry.exe` link shim. `main` already
  fixes that link failure in `7b90108d17`, with MSVC `/ALTERNATENAME` weak
  defaults, and that commit's message explicitly rejects the Cargo-feature
  approach as unable to express "this link has no provider".
- `perry-ui-windows-winui`'s `reorder_child`. Already on `main` from the same
  commit, with an extra `parent <= 0` guard.
- `gc/tests/heap_generation.rs`'s funnel-assert change. `main` already handles
  the profile split with `#[cfg_attr(not(debug_assertions), ignore)]` and an
  in-code note saying not to rewrite the assertion instead.
