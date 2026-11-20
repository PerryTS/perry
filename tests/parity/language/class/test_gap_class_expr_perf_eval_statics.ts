// Per-evaluation statics for class expressions: two evaluations that extend
// DIFFERENT top-level classes must each read THEIR OWN parent's static, not the
// last-registered sibling's (the shared template's parent edge is last-wins).
class P1 { static v = "p1"; static who() { return "P1"; } }
class P2 { static v = "p2"; static who() { return "P2"; } }
function makeChild(p: any) { return class extends p {}; }
const C1 = makeChild(P1);
const C2 = makeChild(P2);
console.log("C1.v:", (C1 as any).v);          // p1 (its own pinned parent)
console.log("C2.v:", (C2 as any).v);          // p2
console.log("C1.who:", (C1 as any).who());    // P1 (inherited static method)
console.log("C2.who:", (C2 as any).who());    // P2
// A third evaluation extending P1 again must still be p1.
const C3 = makeChild(P1);
console.log("C3.v:", (C3 as any).v);           // p1
