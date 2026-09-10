# Next investigation: amortize read guards across a loop

R4's hint restored the ordinary Array/Object-before-JSON order in actual machine code and removed the 20MiB repeat regression, but 20MiB mixed-field reuse regressed 4.49% versus main. Every smaller access pattern also slowed versus R3's earlier window. Do not tune branch hints by benchmark key/index spelling or declare an ordinary-path win from source order alone. R4 is rejected; use R3 as the stronger JSON starting point unless a controlled comparison supports another choice.

Existing infrastructure directly addresses repeated guard overhead:
- crates/perry-codegen/src/stmt/element_shape_loop.rs: match_element_shape_versioned_loop, lower_element_shape_versioned_for.
- crates/perry-codegen/src/expr/element_shape_guard.rs: once-per-loop preheader and per-element residual field loads.
- This clone is admitted only for an effect-free numeric reduction. After lowering, a GC-unsafe-call scan makes unproven fast blocks unreachable. It does not alter global GC policy.
- Current matcher requires arr[counter].field (or a single element binding), a declared class/closed typed element candidate, and raw-f64 field slots. Any JSON arrays, literal indices and modulo indices do not get it. No element_shape.loop blocks appear in the actual access-worker IR.
- Runtime array/element_shape.rs explicitly rejects class_id==0 at lines293 and360. JSON records therefore cannot simply be added to the typed-class matcher. They need a distinct validated data-record contract, not removal of class/type checks.

A bounded first slice could hoist a loop-invariant indexed numeric read in an otherwise effect-free reduction, guarded before the loop. It must check that the loop executes before touching a possibly throwing receiver/index; prove an own data property and numeric result without invoking a getter/coercion; preserve negative/zero-trip loops; and route every unproven shape to the existing loop. Lazy JSON can use a noncollecting projection probe; ordinary JSON data records need an equally strict own-numeric-data probe. A successful primitive value can stay unboxed in a call-free clone. Do not hoist an ordinary getter and then run fallback, which would duplicate its side effects.

A later varying-index clone could query a data-record shape/field offset once and retain per-element shape/descriptor/bounds/numeric guards with a precise side exit. Consider scalar-only native metadata (no new heap-pointer cache/root holder). Class0 JSON record numbers are JSValues, not typed raw-f64 fields; conversion and numeric proof must reflect that distinction. The existing typed-layout flag does not authorize this new layout.

Every change must retain mutation/accessor/prototype/coercion/identity and GC-root stress controls, including a live slow fallback. Ordinary array subclasses, forwarding, holes and length changes cannot be treated as plain Arrays. Keep call-freeness as an enforced property of an entered clone. Both parse/stringify, all current access rows, peak RSS and retained RAM remain part of the eventual acceptance matrix.

Tiny stringify crossover remains unresolved: R3 median +0.73% main, main-codegen+R3-runtime +0.41% main, R3 +0.32% crossover; all9sample ranges overlap. This does not isolate one cause. Do not label it noise or blame GC without additional evidence. Larger-input round-trip/RSS and rotating changing-input costs still require independent work; loop optimization does not fulfill the whole goal.
