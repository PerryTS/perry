# Remaining JSON parse costs: R25 sampling diagnostics

Three bounded profiles of the exact R25 rotating worker, source `01f2878dad8efc92e394c49b88fe0800b871f593`, on the quiet M1 benchmark host. These are instrumented diagnostics, not new CPU/RSS benchmark results. The separately measured R25 rotating comparisons put small-record parse at 1.48x Node / 1.94x Bun, 1 MB ASCII-text-object parse at 1.42x Bun, and 1 MB Unicode-text-object parse at 2.23x Bun.

Every profile has more than 500 workload samples, completes without a watchdog stop, and matches Node on all eight complete input results plus the final result. The window and raw call trees are archived. Inclusive frame groups overlap and cannot be added; the entries below quote self samples in the named frames.

| Workload | Workload samples | Leading self-sample frames |
|---|---:|---|
| Small record | 644 | Object parse: 73 (11.3%); template capture: 43 (6.7%) |
| 1 MB ASCII text object | 759 | String scan: 269 (35.4%); nesting precheck: 200 (26.4%); memory copy: 137 (18.1%) |
| 1 MB Unicode text object | 758 | UTF-16 count: 373 (49.2%); string scan: 163 (21.5%); nesting precheck: 116 (15.3%) |

The large-string fixtures are objects with an id and a large text field. The parser already avoids nesting scans for a scalar root; that shortcut does not apply to these object roots. GC/policy work is also visible, especially in the small-record loop, but this investigation proposes no collector-policy change.

A next candidate is deriving a large unescaped token’s UTF-16 length from the exact source string’s existing count when the bounded surrounding bytes are ASCII. It requires a source payload identity check and a conservative end-boundary check, preserving the runtime’s WTF-8 interpretation. A standalone model tested 3,065,796 byte/Unicode cases: 1,473,719 hints matched the exact runtime scalar counter and 1,592,077 cases declined the hint. The model is not integrated parser validation or performance evidence.

Further work includes parser integration and GC/Unicode/source-slice coverage, then controlled rotating-input timings. It must preserve ordinary token/control validation, malformed JSON rejection, stack-depth protection, and existing fallback paths.
