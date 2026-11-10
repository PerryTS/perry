// A declared static field read by compiled code (`C.x`) and by reflection
// must agree after the property is deleted, redefined as an accessor, or made
// read-only; and a constructor's [[Prototype]] set at runtime is honoured.
class C { static x = 1; static y = 2; static z = 3; }
class P { static fromP = "p"; }
delete (C as any).x;
console.log("deleted", C.x, "x" in C, Object.keys(C).join(","));
Object.defineProperty(C, "y", { get() { return 20; }, configurable: true });
console.log("accessor", C.y, Object.getOwnPropertyDescriptor(C, "y")!.get !== undefined);
Object.defineProperty(C, "z", { writable: false });
try { (C as any).z = 30; } catch (e) {}
console.log("readonly", C.z, Object.getOwnPropertyDescriptor(C, "z")!.writable);
class Q {}
Object.setPrototypeOf(Q, { inherited: "yes" });
console.log("setProto", (Q as any).inherited, Object.getPrototypeOf(Q).inherited);
Object.setPrototypeOf(Q, P);
console.log("setProto class", (Q as any).fromP, Object.getPrototypeOf(Q) === P);
Object.setPrototypeOf(Q, null);
console.log("setProto null", Object.getPrototypeOf(Q), (Q as any).fromP);
class R extends P {}
(P as any).fromP = "p2";
console.log("inherited static write", R.fromP, Object.getOwnPropertyNames(R).includes("fromP"));
(R as any).fromP = "r";
console.log("shadow", R.fromP, P.fromP, Object.getOwnPropertyNames(R).includes("fromP"));
