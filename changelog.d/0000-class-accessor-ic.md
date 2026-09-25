**perf(runtime): cache class getter/setter resolution per receiver shape (#10498).** A `get x()` / `set x(v)` declared in a class body lives in the class vtable, never as an own key, so every inline cache missed it and every `obj.x` / `obj.x = v` re-ran the whole generic path: a getter read walked the IC-miss ladder and a SipHash vtable probe per class level, and a setter store ran the full `OrdinarySet` walk over the prototype objects before `js_object_set_field_by_name`'s vtable walk finally called the setter.

New `object::class_accessor_cache`: a per-thread, direct-mapped `(receiver class id, ShapeId, key) -> vtable getter / setter` table. Entries are **recorded, not re-derived** — written at the one point where the generic path has already committed to calling a vtable accessor for that receiver and key:

- **getters**: the two vtable-getter arms of `get_field_by_name_object_tail`, which is only reached from the full `[[Get]]`.
- **setters**: `js_object_set_field_by_name`'s vtable-setter arm, but only when it is the `CreateDataProperty(Receiver)` tail of a `[[Set]]` walk whose target is the receiver. The walk arms a probe around exactly that `target_set` call, the vtable arm captures what it saw into it, and the arming frame commits the capture after `target_set` returns — so a throw commits nothing, and a direct `js_object_set_field_by_name` caller (whose semantics are not `[[Set]]`'s) never records.

Hits are served from the IC read-miss handler (`get_field_ic_miss_impl`) and from PutValue (`js_put_value_set`, and `js_put_value_set_packed_miss` ahead of its rooting and re-prime). Every hit re-proves: key/class id/ShapeId identity, `proto_validity()` (descriptor installs anywhere including `Object.defineProperty(C.prototype, …)`, `delete`, `setPrototypeOf`, class-prototype registration, structural mutation of marked prototypes) and `vtable_generation()` (accessor/method registration, parent linking), plus the per-object facts a ShapeId does not pin (no own descriptors, no recorded `[[Prototype]]`, not `process.env`/`arguments`, no elements store, not a dictionary; writes also refuse frozen/sealed/non-extensible receivers). Keys answered by name-specific arms (`length`, `size`, `constructor`, `__proto__`, private and index keys, …) are never recorded. The key is the only heap reference an entry holds; it is marked and rewritten by a registered root scanner and pruned through `DEAD_KEY_PRUNES`. `PERRY_CLASS_ACCESSOR_IC=0` turns the cache off for A/B measurement in the same binary.

Measured on the issue's fixture (release runtime, `PERRY_CLASS_ACCESSOR_IC=0` → on, same binary; callgrind instructions per loop iteration, and wall-clock median of 3 at N = 1,000,000):

| variant | instr off | instr on | ms off | ms on |
|---|---:|---:|---:|---:|
| `getter_read2` (2 getter reads) | 7,916 | 901 | 788 | 53 |
| `setter_write2` (2 setter stores + 1 field read) | 41,823 | 677 | 4,935 | 42 |
| `setter_ctor` (`new` + 2 setter stores in the ctor) | 47,155 | 5,948 | 5,738 | 564 |
| `field_ctor` (control) | 5,535 | 5,535 | — | — |

`setter_ctor` is now within 1.1× of `field_ctor` and `setter_write2` within the issue's ≤ ~700-instruction target. `getter_read2` does not reach the ≤ ~400 target: about half of what remains is the emitted read site and its inline inherited-read probe, which run before the runtime is entered; closing that needs a codegen-side accessor IC.

Tests: `object::class_accessor_cache::tests` (record/hit, shape sharing, every invalidation route, the probe's matching and abandoned-probe rules, rooting predicate), `gc::tests::class_accessor_cache_roots` (key relocation rewrite, scanner registration), and `test-files/test_gap_10498_class_accessor_ic.ts` (constructor setters, subclass overrides, many shapes at one site, setter redefinition on the prototype / own shadowing / `setPrototypeOf` after a warm loop, a throwing getter, name-special keys).
