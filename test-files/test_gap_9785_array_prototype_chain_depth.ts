// #9785: indexed Get, HasProperty, and strict Set follow every custom array
// prototype link, stop at null, and preserve ordinary-array inheritance.

function show(label: string, value: unknown): void {
  console.log(label, JSON.stringify(value === undefined ? "undefined" : value));
}

// ── 1. a middle link that must be consulted ──────────────────────────────────
const mid: any = { 5: "from-mid", 7: "mid-seven" };
const protoArr: any = [];
protoArr[3] = "from-protoArr";
Object.setPrototypeOf(protoArr, mid);

const arr: any = [];
arr[0] = "own-zero";
Object.setPrototypeOf(arr, protoArr);

show("depth.own", arr[0]);
show("depth.viaProtoArr", arr[3]);
show("depth.viaMid", arr[5]); // spec: "from-mid" — the skipped link
show("depth.viaMid7", arr[7]); // spec: "mid-seven"
show("depth.absent", arr[9]);
show("depth.hasMid", 5 in arr);
show("depth.hasAbsent", 9 in arr);

// ── 2. a chain terminated with null must stop ────────────────────────────────
(Array.prototype as any)[42] = "default-array-proto";
(Object.prototype as any)[43] = "default-object-proto";

const cutProto: any = [];
cutProto[1] = "cut-one";
Object.setPrototypeOf(cutProto, null);

const cut: any = [];
Object.setPrototypeOf(cut, cutProto);

show("cut.viaCutProto", cut[1]);
show("cut.arrayProtoLeak", cut[42]); // spec: undefined
show("cut.objectProtoLeak", cut[43]); // spec: undefined
show("cut.has42", 42 in cut);
show("cut.has43", 43 in cut);

// A plain array still inherits both, which pins that the leak above is about
// chain termination and not about the indices being absent altogether.
const plain: any = [];
show("plain.arrayProto", plain[42]);
show("plain.objectProto", plain[43]);

// ── 3. strict [[Set]] must consult the same chain the [[Get]] walks ──────────
// A non-writable inherited index makes a strict assignment throw; the owner
// search has to find it through the SAME depth the read uses.
const roProto: any = {};
Object.defineProperty(roProto, "6", { value: "readonly", writable: false, enumerable: true });
const midArr: any = [];
Object.setPrototypeOf(midArr, roProto);
const target: any = [];
Object.setPrototypeOf(target, midArr);

let threw = "no-throw";
try {
  "use strict";
  const assign = new Function("o", '"use strict"; o[6] = "written";');
  assign(target);
} catch (error) {
  threw = (error as Error).constructor.name;
}
show("strictSet.threw", threw);
show("strictSet.value", target[6]);
show("strictSet.own", Object.prototype.hasOwnProperty.call(target, "6"));

// Clean up so the trailing summary is not polluted for other readers.
delete (Array.prototype as any)[42];
delete (Object.prototype as any)[43];
console.log("array-proto-depth-v1:done");
