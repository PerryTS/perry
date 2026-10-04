**Classes evaluated more than once are much cheaper to use: `new` and method calls on a fresh class now cost about what they cost on a shared class** (Refs #11769, #10616).

A class expression or declaration that is evaluated more than once gives every later evaluation its own class object, prototype and instances (#11780, #11759). Using those classes went through slow paths: every `new` re-derived the instance's shape from scratch and re-marked the prototype, every field read and write in the class's methods missed its guard and fell back to a by-name lookup, every method call through an untyped receiver probed for an own property by name, and every static method call missed its call site. This change makes those paths take the template's precomputed shapes.

- **The class object names its template.** A per-evaluation class object's first own key (`#<perry:class-template>`, hidden like the other internal keys) holds its template cell. The runtime paths that start from a class object (building its prototype, adding an internal key, `new`) read the template from the object itself. The template-cell lookup through the class registry lock, and `js_register_class_template_cell`, are deleted.
- **Instances are born linked.** An instance of one evaluation carries its class's birth shape at that evaluation's prototype. The template cell remembers the last link `new` made: the birth shape and width, the prototype's identity and the linked shape. The next instance of the same evaluation is allocated directly in the linked shape (one allocation, one stamp) without a birth shape, a prototype mark or a shape intern. The first instance of another evaluation is allocated in the recorded birth shape and linked the ordinary way. Instances are now born in their class's declared keys, as `new` of a shared class is, so their own keys now come in declaration order, as Node lists them (`Object.keys` and `JSON.stringify` of such an instance listed them in another order before).
- **A prototype link keeps the lanes.** Moving an object to a new prototype keeps its own slots' representation lanes, so a linked instance keeps its class's numeric (`F64`) lanes.
- **Field guards admit linked instances.** A class field read or write accepts an instance whose shape is the class's birth shape at a recorded prototype of the same class: the same own keys in the same slots with the same lanes.
- **Linked instances are as wide as the class's instances grow.** The recorded link includes the width its birth shape was allocated at (a birth shape carries its width as its live inline slots). Once the class's instances have been learned to grow wider than that (a constructor that adds keys spills on the first instance), `new` allocates at the learned width and records the wider link, so the keys a constructor adds stay inline. Keeping the first instance's width would spill every later instance's keys to overflow storage.
- **Static calls.** A method call site serves a class object's own static methods (own keys only; what a class object inherits follows its pinned parent).
- **Smaller costs.** A class object's non-pointer slots are written with its allocation; a `new` through a class whose template has no heritage skips the Promise and fetch backing checks.

**Cost** (instructions per operation, main 600f8f29d → this change; node from the 2026-10-03 sweep):

| row | main | this change | node |
|---|---|---|---|
| #11769 `new F()` on an existing fresh class (`freshnew`) | 9,201 | 5,846 | 10 |
| #11769 method call on its instance (`freshcall`) | 1,711 | 1,627 | 10 |
| #11769 evaluate + `new` + call (`evalnew`) | 38,121 | 27,664 | 30,725 |
| #10616 `new K()` (`ctor`) | 13,288 | 10,686 | 34 |
| #10616 static call (`stat`) | 5,360 | 505 | 28 |
| later evaluation: `new` through an untyped value (`newcall_later`) | 9,680 | 6,243 | |
| shared class `new` / call through an untyped value | 68.6 / 7.0 | 68.5 / 7.0 | |

The shared-class rows are unchanged. tsc (n=5): instructions -0.39%, full collections 76 in both, .text -640 B. Zod, qs, commander and hello: instructions within +-0.04%. tsc RSS is +0.74% (n=20) because of transparent-huge-page residency, not allocation (see the PR).

**Not done** (follow-ups): a later evaluation still costs ~25k instructions (prototype mark mints a per-prototype shape, one function object per method, the instance's private-brand metadata record), a static method still allocates its function object per evaluation (~670 instructions each), and #10616's constructor spends half its time writing its captured variable through the generic `PutValue` path (the boxed capture's holder is typed `Any`).
