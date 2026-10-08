Retire `CLASS_PROTOTYPE_METHODS` and its enumerability side table (Refs #10502).
Prototype assignments now lower to ordinary property stores on their actual
receiver. `defineProperty` and deletion likewise edit the prototype's physical
properties, and inherited calls and reads observe its holder shape. This fixes
function-prototype replacement, writes through saved prototype aliases, and
stale method-value reads after replacement or deletion.

Remove the table's lookup helpers, GC scanners, snapshot slots, memory census
rows, and prototype backfills. The legacy registration ABI stores ordinary
properties. Declared prototypes use the existing private prototype shape lineage,
and the ordinary slot-store funnel retires current main's direct-call guards
until S5 replaces them with holder guards.
Prototype birth installs its declared fields before marking the completed
holder, so initialization preserves those guards. Rooted operands survive
the allocating mark and subsequent shape learning.

`test_gap_10502_s6.ts` covers function-prototype replacement, class patches after
instances exist at monomorphic and megamorphic sites, super calls, method-value
identity, a prototype getter, deletion revealing the inherited method, and
static patching, methods beyond the ConstFn slot limit, and a store site primed
on an ordinary object before writing the prototype. Validation and instruction/RSS A/B evidence are recorded in
the lane report.
