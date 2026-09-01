`PERRY_GC_VERIFY_MARK` already owns the direct signature of a dropped old→young
edge: `verify_minor_unmarked_young_children_report` walks every old parent and
reports a child slot naming a sweep-eligible object that is UNMARKED — about to
be freed while its parent survives. It was wired only into `cycle.rs`'s
non-copying minor. The **copied** minor is the operative cycle on every workload
that reaches that path, and it is the collector that does the moving; it ran
without one. So on exactly the cycle where a dropped edge matters it was
invisible, and the first observable was a garbage `GcHeader` hundreds of
collections later, in an unrelated function.

Wires it there, and adds a second check for an invariant nothing was asking.
`verify_array_pointer_slots_enumerated`: the collector's own slot enumeration
must reach every array element that holds a heap reference.
`heap_payload_slot_selection` answers with a full scan, an all-pointer claim, or
a per-object mask — the first two cannot under-report, the mask can. When it
does, the omitted element is a live edge that marking never marks and the
evacuation rewrite never rewrites; the child is swept while its owner still
names it, and the collector reads the recycled bytes as a header one or more
cycles later.

Arrays only, deliberately: an array's payload is `length` elements at a known
offset, so "which words are supposed to be reachable" needs no layout
interpretation, and the check cannot inherit the bug it is looking for. Both
probes report their own subject counts, so a cycle on which they had nothing to
check says so rather than reading as a clean bill.

This is the instrument, not the fix. On the claude-code bundle under
`PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1` it names the defect behind
#9261 in one run — an object's spill (overflow-field) buffer whose mask covered
every `POINTER_TAG` element and omitted three live `STRING_TAG` ones
(`mask=0xc7fc live=0xfffc missing=0x3800`) — where the bare abort was
`[gc-pin-latch] FATAL … obj_type=10 size=1347565393`, i.e. the ASCII `QTLP`,
string payload bytes read as a header, ~200 collections downstream.

Tests are a sabotage pair: a mask-described array is verified clean and the
clean verdict is required to have examined at least one pointer element, so it
cannot pass vacuously; then a pointer is published at the append position
WITHOUT the layout note every store path performs — the exact state found in the
wild — and the check must see it, at the right index, naming the right child.

Nothing runs outside `PERRY_GC_VERIFY_MARK`: no new knob, no production path
touched.
