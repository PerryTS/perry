# Object-write coverage matrix (#6812)

Status: MEASURED (triage pass) — path column derived from code gates on
`main` @ `8bda0351e`; numbers are medians of 3 alternated runs, node v26.3.0,
macOS arm64, release + default pipeline, checksums identical on every cell
(raw samples: `matrix_raw_2026-07-25.csv`; ambient load ~7 — cells promoted
to PR evidence get the full 15-run idle protocol). Goal per the raised bar:
**beat node on every row** or justify the fallback by semantics / measured
lack of benefit.

## The three write mechanisms and their gates

1. **Whole-loop numeric clone** (`stmt/loops.rs::match_object_array_write_loop`
   → `lower_object_array_write_versioned_for`; preflight
   `js_object_array_numeric_write_guard`): constant-counted nested `for`s,
   inner from 0 (bound ≤ 16M), one immutable `const o = objs[i]` alias, ≤ 4
   static-key writes to it, RHS numeric expressions over counters/constants,
   no calls/allocations/labels; preflight proves dense same-shape prefix,
   writable own slots, layout. Strongest path: call-free, barrier-free body.
2. **Static-key write PIC** (`expr/proxy_reflect.rs::lower_put_value_static_write_ic`,
   4-entry polymorphic since #6823; miss/priming
   `proxy/put_value.rs::js_put_value_set_ic_miss`): static (interned/const)
   key, target ≡ receiver expression, safepoint-free RHS, heap object,
   non-forwarded, blocking flags clear (frozen/sealed/no-extend/descriptors/
   typed-intact), `object_type == REGULAR`, **`class_id != 0`**, shape-token
   match (id-or-keys discriminated), slot in bounds.
3. **Runtime fast path** (header-first classification + existing-own-data
   overwrite routing in `js_object_set_field_by_name` / `put_value_set`):
   everything else that is still an ordinary data write.

## Matrix

Ratio = perry/node median (fill from measurement; `<1` = beating node).

| cell | shape of the write | path (gate) | perry ms | node ms | ratio | verdict |
|---|---|---|---:|---:|---:|---|
| w0_canonical | 2 static writes, const alias, nested const `for`s, `+`/`-` RHS | whole-loop clone | 6 | 9 | **0.67** | BEATS node |
| w1_three_writes | 3 static writes, same loop | whole-loop clone (≤4 fields) | 7 | 9 | **0.78** | BEATS node |
| w2_one_write | 1 static write | whole-loop clone | 5 | 7 | **0.71** | BEATS node |
| w3_mul_rhs | RHS uses `*` | NOT clone (`Add\|Sub` only) → PIC | 47 | 7 | 6.7 | GAP: extend numeric matcher |
| w4_dyn_bound | inner bound `objs.length` | NOT clone (const bounds only) → PIC | 51 | 8 | 6.4 | GAP: length-stable versioning |
| w5_while | same body, `while` form | NOT clone (`for`-only) → PIC | 46 | 7 | 6.6 | GAP: while normalization |
| w6_call_in_body | user call inside body | NOT clone → PIC + call | 100 | 7 | 14.3 | GAP: call-tolerant clone / inlining |
| w7_mut_alias | `let o = objs[i]` | NOT clone (const alias only) | 70 | 8 | 8.8 | GAP: SSA no-reassign proof |
| w8_helper_mono | writes inside helper fn, mono | static-key PIC hit (~10 ns/write) | 47 | 8 | 5.9 | GAP: call overhead + guard chain vs node's full inlining |
| w9_poly2 | 2 shapes through one site | PIC entries 1–2 | 27 | 6 | 4.5 | GAP: same as w8 + entry chain |
| w10_poly8 | 8 shapes through one site | PIC exhausted → runtime miss | 27 | 19 | 1.4 | close; megamorphic path is decent |
| w11_stable_dynkey | `o[k]`, `const k = "c"` | clone (const-string local = static) | 6 | 8 | **0.75** | BEATS node |
| w12_arb_dynkey | rotating keys from array | generic | 84 | 18 | 4.7 | GAP: GC-safe dynamic-key cache |
| w13_int_key | `o[7]` on plain object | generic numeric-as-property | 160 | 13 | 12.3 | GAP |
| w15_append_build | fresh `{}` + 6 assigns (builder) | generic transitions; `class_id==0` blocks PIC | 1489 | 8 | **186** | TOP GAP — ubiquitous pattern |
| w16_overflow_slot | writes past inline capacity | runtime (PIC bounds reject) | 4150 | 23 | **180** | TOP GAP — wide objects |
| w17_alloc_rhs | allocating RHS (`"s"+i`) | NOT PIC (safepoint-free rule) | 26 | 18 | 1.4 | close; revisit only with receiver-reload design |
| w18_class_inst | class instances via `any` | PIC | 6 | 12 | **0.50** | BEATS node (best row) |

### Reading of the triage pass

- **Beating node already (5 rows):** everything the whole-loop clone covers,
  plus class instances through the PIC. The #6811/#6823 architecture wins
  when it applies.
- **Two catastrophic rows (~180×), both ubiquitous in real code:**
  the builder pattern (fresh `{}` + property assigns — every API response,
  parser, ORM row) and wide-object overflow-slot writes. These dominate the
  broad-impact ranking by an order of magnitude.
- **A coherent 5–9× family** (w3–w8): each is one narrow eligibility rule of
  the clone matcher; per-write PIC at ~10 ns/write is the shared floor. The
  fix is widening clone eligibility, not touching the PIC.
- **Megamorphic (w10) and allocating-RHS (w17) are near-node already** —
  deprioritize.

Not modeled as micros (justified fallbacks unless measurement says otherwise):
symbol keys (separate table semantics), frozen/sealed/descriptor receivers
(semantic guards must fire), proxies/exotics (trap semantics), typed-layout
locked objects (raw-f64 invariants).

## Broad-workload guard rails

Every optimization slice must hold or improve: `babel_init` whole-process,
`benchmarks/app-patterns`, `honest_bench` — no benchmark-shaped regressions
hiding behind micro wins.
