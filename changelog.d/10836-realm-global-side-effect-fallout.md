Eight tests depended on a side effect that v0.5.1627 correctly removed, and one
of them hid the other seven.

`resolve_prototype_addr` used to **materialize the realm global** as a side
effect of answering "is this `Object.prototype`?". v0.5.1627 stopped it (the
bootstrap cost ~5 ms on the first `setTimeout`, skewing timer deadlines) and it
now honestly answers `0` on a thread that has no `globalThis`. Several tests
were silently relying on the old side effect to build their realm.

**Why it was not caught then.** One of the eight is
`bun_compat::plugin::tests::calls_setup_for_objects_and_functions_without_running_hooks`.
Its `expect("builder hook")` panics inside an `extern "C"` function, which
cannot unwind, so the process **SIGABRTs** — taking the remaining ~3,600 tests
of `perry-runtime` with it. The suite emitted no `failures:` block and no
`test result:` line, so it registered as `rc=101` with an empty failing set.
Trains 248, 249, 250 and 251 all ran with truncated runtime coverage.

Bisected to `c151aadaf2` exactly: all eight pass at `dbf948879f`, the commit
before it.

Each is fixed where the assumption lives, not at the symptom:

* **7 × `intl::segments_view::view_mode_tests`** — one line in the shared
  `grapheme_segmenter()` helper. It goes through the runtime's real constructor
  path, and a real `new Intl.Segmenter(...)` always runs in a realm that has a
  `globalThis`.
* **`gc::tests::runtime_roots::prototype_addr_cache::a_second_agents_prototype_addresses_are_its_own`**
  — each agent now bootstraps its realm explicitly, inside the serialized
  section the test's own comment already describes ("`GLOBAL_THIS_PTR` … is
  written by each one"). It was relying on the accessors to do it.
* **`bun_compat::plugin`** — establishes its realm before forcing the collection
  it is about.

That GC test is the reason the blast radius was knowable: it asserts
`addr != 0` *specifically* so that "two agents resolved different addresses"
cannot be satisfied by two agents that resolved nothing. Separating the two
paths to a green verdict is what let it report the real problem instead of
passing.

Full `perry-runtime` suite after: **4218 passed, 0 failed, 0 SIGABRT** — its
first clean completion since train 248.
