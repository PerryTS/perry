**Delete the `tokio-wait-driver` A/B arm (turnloop P8's reserved step).**

`tokio-wait-driver` was the baseline arm of the tokio-vs-turnloop measurement:
a build in which no agent got a `turnloop::Loop`, so every turnloop fast path
declined and every binding fell back to its legacy tokio transport. Both
feature declarations said "Deleted in P8"; this is P8.

Removed: the `perry-runtime` and `perry-stdlib` feature declarations, 38 `#[cfg]`
arms across 8 crates (6 tokio-arm items deleted outright, 9 turnloop arms made
unconditional, 23 compound predicates narrowed — `all(not(wasm32),
not(feature))` to `not(wasm32)`, `any(wasm32, feature)` to `wasm32`), and both
runtime `cfg!` branches, which had become statically false. The CI step that
existed only to keep the arm compiling goes with it.

**What this changes beyond dead code: the remaining decline blocker is now
singular.** Every plan-A and plan-B edge in `scripts/tokio_inventory.json`
recorded the same PAIR of reasons a turnloop path can be unavailable — the A/B
arm, which compiles no agent loop by construction, and a host where
`Loop::new` failed. One of those two is now gone, so each of those edges is
held open by exactly one cause. The annotations say so rather than quietly
dropping a list item; whether a `Loop::new` failure justifies carrying a whole
tokio transport is the next decision, and it is now the only one.

The A/B itself is NOT lost, but it is no longer a feature flag. The harness had
already moved its tokio arm to a pre-migration COMMIT (`--arm-tree
tokio=<path>`), because once the servers, clients and database drivers stopped
going through tokio the feature no longer selected "Perry on tokio" — it
selected a third thing that was nobody's baseline. `scripts/turnloop/server_ab.py`
now REFUSES to build a tokio arm from this tree rather than silently producing a
second turnloop arm and reporting a 0% delta as a result. The authoritative
numbers stand at `650ea6d661`.

`scripts/turnloop_p0_loop_stats.py` loses its `--arm` switch for the same
reason. The `[perry-loop] driver=` line stays: a run that names its driver is
checkable, and a subject that is merely assumed is the failure this whole
family of instruments exists to prevent.

Phase reports under `docs/turnloop/` and earlier `changelog.d/` fragments still
describe the arm. They are records of what was true when each lane ran and are
deliberately left alone.
