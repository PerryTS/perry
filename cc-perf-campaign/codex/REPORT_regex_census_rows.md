# RegExp heap-census rows

## Branch and commits

- Branch: `diag/regex-census-rows`
- Base: `a93908a6cc684d511a0af8561b65be4f0536fbd4` (`fork/perf/regex-literal-site-test`)
- Common implementation: `49f551f05` (`feat(diagnostics): account regex tables in heap census`)
- #9958 site-table integration: `5e8079138` (`feat(diagnostics): add literal-site regex census rows`)

Both implementation commits are diagnostic-only. No construction, matching,
cache-maintenance, or collection path calls the new walkers. The walkers run
only while an explicitly requested heap census is being assembled.

## Emitted rows and byte derivation

Every row is a JSON object in the census's existing `side_tables` array:
`{"table":"regex.<table>","entries":<n>,"bytes":<n>,...}`.

- `regex.pointers`: `entries` is `REGEX_POINTERS.len()`; `bytes` is the
  HashSet bucket/control estimate; `live_headers` is the marked-or-pinned
  RegExp-header count at sweep entry.
- `regex.program_cache`: the 512-entry `REGEX_CACHE`; `bytes` is HashMap
  storage plus one de-duplicated lower-bound estimate per compiled `regex`
  program. `compiled_programs`, `opaque_program_bytes`, `cleared`, and
  `evictions` are included. The event counters read the already-existing
  `PERRY_REGEX_DIAG` state and are zero when that diagnostic is off.
- `regex.fancy_cache`: the 512-entry `FANCY_CACHE`; HashMap storage plus one
  de-duplicated `fancy_regex::Regex` lower bound.
- `regex.repeat_cache`: the 512-entry `REPEAT_MATCHER_CACHE`; HashMap storage,
  the public wrapper/Arc lower bound, and visible capture-name Vec/String
  buffers. The opaque `regress::Regex` heap graph is not exposed.
- `regex.validated_patterns`: the 512-entry validation map, including map
  storage and owned pattern/flag String capacities.
- `regex.content_cache`: the 1,024-entry content map, collision-bucket Vec
  capacities, owned pattern/flag text, `Programs` Arc allocations, and matcher
  lower bounds not already charged to an engine cache. It reports
  `pinned_programs` and `opaque_program_bytes`.
- `regex.literal_sites`: the 1,024-slot literal-key Vec, including its full
  `Option<Entry>` capacity. Its Arc text allocations are shared with the
  content table and are not charged twice.
- `regex.site_table`: the #9958 site-to-rooted-header map. `sites` and
  `rooted_headers` are exact. `rooted_header_bytes` is reported but explicitly
  outside `side_table_bytes`, because those 56-byte headers are already in the
  GC live-heap census. `pinned_programs` and `pinned_program_bytes` are gross,
  unique program-bundle retention measurements, including programs shared
  with content/engine caches; that gross field is explicitly outside the
  additive total to prevent double counting. `exclusively_attributed_programs`
  and `attributed_program_bytes` are the de-duplicated subset owned nowhere
  else and are inside this row's `bytes` and `side_table_bytes`.
- `regex.active_factory_sites`: the transient #9958 authorization-stack Vec
  capacity and current entries (normally empty when a synchronous census
  runs).
- `regex.expando_owners`: the RegExp owner share of the mixed exotic-expando
  HashMap plus each RegExp-owned property Vec and key-string capacity;
  `owners` and `properties` are included.
- `regex.matcher_kinds`: counts `unbuilt`, `standard`, `fancy`, and `repeat`
  headers. `bytes=0` and `bytes_inside_side_table_bytes=false`, because the tag
  resides in each already-counted `RegExpHeader` rather than a side table.

The regex engines do not expose their complete compiled heap graphs. All
program rows therefore carry
`program_bytes_estimate="opaque_inline_lower_bound"`: Arc counters, the public
wrapper value, exposed source text, and exposed auxiliary buffers are counted;
unexposed automata/program allocations are not guessed. The program-count
fields remain exact and are the decisive signal if the RX2 delta is primarily
opaque engine storage.

## Reconciliation

`side_table_bytes` remains an explicit census estimate, not an allocator tag.
The assembly now computes:

`side_table_bytes = non_regex_side_table_bytes + regex_side_table_bytes`.

Legacy regex tuples from #9958 are removed from the ordinary row stream before
summing. The rich regex rows are serialized once, while
`regex_side_table_bytes` is built by an independent second diagnostic walk of
the same registered table inventory. Consequently the sum of every emitted
`regex.*` row's `bytes` must equal `regex_side_table_bytes`; omitting a row does
not silently shrink the attributed total.

Gross `regex.site_table.rooted_header_bytes` and `pinned_program_bytes` are
labelled outside the additive total. The row's table bytes and
`attributed_program_bytes` are inside it. This preserves reconciliation while
still exposing the causal site-pinned working set side by side.

## Tests and sabotage

- `census_prints_regex_rows_that_reconcile_with_side_table_total` constructs
  six distinct regex sources, executes them, evaluates one direct #9958
  literal site twice, serializes and parses the direct census document, and
  asserts pointer entries >= 6, site/root/header/program counts >= 1, the full
  row inventory, `sum(regex row bytes) == regex_side_table_bytes`, and
  `side_table_bytes - non_regex_side_table_bytes == regex_side_table_bytes`.
  Its sabotage assertion removes one non-zero row contribution and proves the
  independent attribution total no longer reconciles. Deleting one row from
  registration therefore fails the equality assertion.
- `census_regex_rows_are_zero_cost_when_not_requested` resets a cfg(test)
  census-walk counter, constructs and matches a regex, proves the counter is
  still zero, invokes the census directly, and proves it advances. Adding a
  per-construction bookkeeping call makes its zero assertion fail.

## Gates

Every reported Cargo test/build invocation used `-j4` and the campaign build
lock, and launched only after `df -g /` reported at least 12 GiB.

- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1 census`:
  final tree, 20 passed, 0 failed, 3,261 filtered out.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1 regex`:
  final tree, 130 passed, 0 failed, 3,151 filtered out.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1`:
  the pre-split implementation passed through Cargo (3,276 passed, 0 failed,
  4 ignored); the exact final Cargo-built executable was then run directly
  after the source-only commit split and passed 3,277, 0 failed, 4 ignored.
  A final no-op Cargo wrapper retry was prohibited because its guarded
  precheck reported 10 GiB after another lane's build.
- `cargo build --release -p perry-runtime --features wasm-host -j4`:
  the combined implementation before its source-only commit split passed in
  4m34s. A final-tree refresh was prohibited by the same 10 GiB precheck; the
  final default-feature release test target compiled without warnings.
- Direct `rustfmt --edition 2021 --check` on every touched Rust file: passed.
- `git diff --check`: passed.
- `scripts/check_file_size.sh`: passed; no Rust source exceeds 2,000 lines.
- `python3 -m json.tool scripts/gc_runtime_root_holders.json`: passed.
- `python3 scripts/gc_runtime_root_holders.py`: passed: 1,372 declarations,
  596 scanner-reached, 357 classified, 414 frontier-pinned, 152 scanners.
- `python3 scripts/gc_runtime_root_holders.py --self-test`: passed: 90 planted
  declarations and 357 inventory entries.

One non-mutating `cargo fmt --all -- --check` attempt was mistakenly allowed
to continue after its chained precheck printed 8 GiB; it reported only the two
formatting changes then applied. The final formatting gate was rerun directly
with `rustfmt --check` on every touched Rust file and passed. No build/test was
started below the floor.

The GC snapshot pin was re-audited because `gc/census.rs` and the module list
changed. `PASS1_MARKED` is still taken out of TLS before `take_census`; all new
regex walks occur afterward, and neither boundary nor intervening cycle flow
changed. No thread-local was added.

One initial full-suite run under a PTY-like harness state had 3,275 passing and
one environment-dependent failure in
`tty::tests::columns_undefined_when_not_tty` (it read terminal width 80). The
exact test passed in a non-TTY process, and the subsequent non-TTY full rerun
passed 3,276 with 4 ignored. The final-tree result above supersedes that
diagnostic history.

## Origin/main application check

The implementation was split at the stack boundary. In a disposable checkout
of `origin/main` at `8b7dc3342b22fe6270739c8d51585c3d2cdfa618`,
`git cherry-pick --no-commit 49f551f05` completed with no conflicts. The
excluded `5e8079138` commit contains the #9958 rooted-site row plus the
#9918/#9958 content/literal cache-layout accessors and the branch-specific GC
snapshot pins. Thus the common regex-table census applies cleanly to main;
the site-table integration is intentionally the one omitted stack row.

## PerryMaster request

On `app-main7rx` and `app-main7` (the still-running RX2 arm and control), run
one 120-second idle interval followed by one SIGUSR2 heap census on each. Send
the complete `regex.*` rows and the three side-table totals side by side:
`side_table_bytes`, `regex_side_table_bytes`, and
`non_regex_side_table_bytes`.

In particular compare `regex.site_table.sites`, `rooted_headers`,
`pinned_programs`, `pinned_program_bytes`, and `attributed_program_bytes`, plus
the 512-entry engine-cache program counts. The expected explanation for RX2's
88.3 MB versus 82.4 MB (+6 MB) is roughly 550 site-pinned programs beyond the
512-entry engine cache. If that population is present, the gross site-pinned
lower bound/count identifies it even when shared ownership assigns additive
bytes to another regex row. If it is not present, the side-by-side reconciled
rows identify which other regex table grew; if no regex row explains the
delta, that is the finding and the residual belongs outside regex attribution.
