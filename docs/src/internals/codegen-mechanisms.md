# Codegen mechanisms and workload evidence

An optimization's benchmark result does not establish that another program
uses the same generated path. Record the admission rule, the workload and
compiler snapshot, and evidence of emitted code before attributing a result
to that mechanism.

The [mechanism index](https://github.com/PerryTS/perry/blob/main/scripts/codegen_mechanisms.json)
starts with the per-site concat cache from
[#9514](https://github.com/PerryTS/perry/pull/9514), following the correction in
[#9824](https://github.com/PerryTS/perry/issues/9824#issuecomment-5554137476).
It is an evidence index, not a complete inventory or a new CI gate. Missing
entries mean unrecorded. Each entry names its lowering, runtime helper,
admission conditions, workload expectations, observations, and existing
regression tests. Refresh observations when the compiler, bundle, flags, or
proof changes; a historical negative is not a permanent property of an app.

## Per-site concat cache

The [lowering](https://github.com/PerryTS/perry/blob/main/crates/perry-codegen/src/concat_site_cache.rs)
requires a string literal on the left in HIR and one of these right operands:

| Right operand | Admission proof |
| --- | --- |
| Compile-time integer | Value in `0..=255`, including supported constant arithmetic and integer-constant locals. |
| Loop-induction local | Proven interval with a nonnegative lower bound and upper bound at most 255. |
| `x % C` | Compile-time integer modulus `C` in `1..=256`; negative remainders fall back at runtime. |

The admission limit is **255**, but each site has only **32 slots**. At runtime,
an integral numeric value in `0..31` can hit a filled slot. The fill arm calls
`js_string_concat_site_value`; the plain arm handles values outside the table.
Ordered comparisons reject NaN and boxed non-numbers. An unproven site keeps
the ordinary fused helper and process-wide memo without this per-site diamond.
`PERRY_CONCAT_SITE_CACHE=0` disables the lowering **when compiling the program**.

The counted-loop shape in
[`bench_object_property.ts`](https://github.com/PerryTS/perry/blob/main/benchmarks/suite/bench_object_property.ts)
is admitted: `"field_" + j` has `j` in `0..19`. Fresh object compilation with
the codegen at `d36a1af0c` produced three tables and three fill call sites;
disabling the cache produced none. Both builds retained the ordinary helper.
This confirms applicability; it does not add a timing measurement to #9514.

The #9824 report found zero fill-helper executions in three runs of the
compiled `cli_2.1.112.js` bundle and no fill-helper symbol in its inspected
binary. Its roughly 8,600 concatenations per reply do not by themselves meet
the admission proof. The record therefore expects no emitted path for that
**reported snapshot**, not for every version of Claude Code. This is expected
workload selectivity, not evidence that the cache is broken. The rule also
admits constants and bounded remainders; it is not restricted to counted loops.

## Recheck a workload

From the repository root, using the compiler whose behavior you want to audit:

```sh
PERRY_NO_CACHE=1 PERRY_NO_AUTO_OPTIMIZE=1 PERRY_LLVM_KEEP_IR=1 PERRY_CONCAT_SITE_CACHE=1 \
  perry compile benchmarks/suite/bench_object_property.ts \
  --no-link --keep-intermediates -o /tmp/concat-on.o 2>/tmp/concat-on.log
PERRY_NO_CACHE=1 PERRY_NO_AUTO_OPTIMIZE=1 PERRY_LLVM_KEEP_IR=1 \
  PERRY_CONCAT_SITE_CACHE=0 \
  perry compile benchmarks/suite/bench_object_property.ts \
  --no-link --keep-intermediates -o /tmp/concat-off.o 2>/tmp/concat-off.log
nm -u /tmp/concat-on.o | rg 'js_string_concat_site_value'
nm -u /tmp/concat-off.o | rg 'js_string_concat_site_value'
```

The last command should have no match (exit 1). `PERRY_NO_CACHE` forces fresh
code generation; `--no-link` makes this an object-level check. For a module
graph, inspect every generated object and every retained IR path in the log.

Open the file named by each `kept LLVM IR:` log line. Count **call/invoke
instructions** to `@js_string_concat_site_value(` and definitions of
`@perry_concat_site_*` globals. A `declare` line is not a call site; searching
for the helper name alone gives a false positive. Confirm the disabled arm
still calls `js_string_concat_value_box` so the negative control is meaningful.

Keep these evidence levels separate:

- Retained pre-optimization IR shows whether codegen emitted the lowering.
- An object reference shows that a call survived compilation. Its absence
  alone cannot distinguish non-emission from later dead-code elimination.
- Linked-binary symbol inspection must account for stripping and linkage.
- A counter at the helper entry measures fill-helper executions, not inline
  cache hits. Zero executions alone does not prove the lowering was absent.

The [existing compiler tests](https://github.com/PerryTS/perry/blob/main/crates/perry/tests/concat_site_cache.rs)
pin positive and negative admission, the disable switch, and Node parity under
evacuation. The [runtime lifecycle test](https://github.com/PerryTS/perry/blob/main/crates/perry-runtime/src/gc/tests/concat_site.rs)
checks that collection rewrites a filled slot. These cover the public admitted
shape even when the application bundle legitimately has no eligible sites.
