### Fixed

- **The prototype registry's fast-path latch was one-way, so a single `Object.setPrototypeOf` disabled it for the life of the process (#7737 item 1).** `OBJECT_PROTOTYPES_NONEMPTY` is set when a non-meta-capable owner records a prototype and was never cleared — **including by `prune_dead_object_prototype_owners` when its `retain()` drains the map back to empty**.

  That became load-bearing with #7733, which added a third reader: the evacuation move hook `object_static_prototype_owner_moved` consults the latch once per moved object to skip a process-global `Mutex<HashMap>` and a SipHash lookup (2.5 M of each on `retain.ts`). So one incidental re-prototyping early in a run — of a `RegExp`, say — silently forfeited that win for every subsequent evacuation, with no signal that it had happened.

  This is **#7510's finding recurring**: *"one immortal side-table entry nullified every `is_empty()` fast path"*, now with a third victim.

  **The clear could not simply be added, and the reason is the interesting part.** The latch was stored *outside* the mutex, deliberately before the insert, so that a reader observing it never misses a committed entry. Clearing under the lock against a set outside it loses entries:

  1. writer stores `true`;
  2. pruner takes the lock, retains to empty, clears the latch;
  3. writer takes the lock and inserts.

  — leaving a non-empty map with the latch false, which every reader skips. The set therefore moves **under the same mutex**, still before the insert, which serialises (1) and (3) against (2) and makes the interleaving impossible. The publish property is unchanged: a reader that sees `true` takes the lock and so sees whatever the writer committed. No extra cost — that path acquired the lock on the next line anyway.

  The regression test's load-bearing assertion is the **last** one, that the latch comes back down; everything before it passes with the bug present. Verified by removing the clear: *"the registry is empty but the latch is still armed, so every evacuated object keeps paying the mutex + SipHash lookup for the rest of the process"*.
