// #5592 (test262 language/class tail): two anonymous class EXPRESSIONS
// assigned to the same binding both infer the binding's name (`C`). The HIR
// registers classes by name, so the second body used to alias onto the first
// class's ClassId — silently dropping it (`C` kept pointing at the first
// class). The fix gives the second expression a unique registration key while
// preserving its user-visible `.name`.
//
// A second, intertwined bug: a computed accessor key (`get [expr]()` /
// `set [expr](v)`) was never tracked as an accessor, so `C.prototype.<key> = v`
// routed to a name-keyed prototype monkey-patch (resolved to the FIRST class)
// instead of the generic, runtime-evaluated setter-invoking path. The Test262
// case `accessor-name-inst-computed-in` (an `in` expression as the computed
// accessor name) is the canonical trigger.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- distinct classes on one binding keep distinct identity + bodies ---
let C: any;
C = class { m() { return 1; } };
const first = C;
C = class { m() { return 2; } };
const second = C;
console.log("distinct identity:", first !== second);
console.log("first.m:", new first().m());
console.log("second.m:", new second().m());

// --- anonymous class expressions still infer `.name` from the binding ---
const Named = class { x() { return 0; } };
console.log("inferred name:", Named.name);

// --- computed accessor name via `in`, getter then setter on the same var ---
const empty = Object.create(null);
let D: any;
let value: any;
D = class { get ["x" in empty]() { return "via get"; } };
console.log("get:", D.prototype.false);
D = class { set ["x" in empty](param: any) { value = param; } };
D.prototype.false = "via set";
console.log("set:", value);

// --- non-`in` computed accessor, same reassign-and-write pattern ---
const key = "k";
let E: any;
let captured: any;
E = class { get [key]() { return "G"; } };
console.log("read:", E.prototype.k);
E = class { set [key](v: any) { captured = v; } };
E.prototype.k = "S";
console.log("written:", captured);
