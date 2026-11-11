// #10511: a loop whose bitwise operators read locals with no function-scope
// Number proof runs in a clone versioned on one entry test per local. Every
// case below must match the ordinary lowering: the values ToInt32 treats
// specially, entry values that are not Numbers (the slow clone), writes in
// the condition and update clauses that withdraw a local from the proof,
// early exits (break / return / throw) that must leave each local's real
// value behind, and BigInt operands that must keep throwing TypeError.

class St {
  A: number = 0x6a09e667 | 0;
  B: number = 0xbb67ae85 | 0;
  C: number = 0x3c6ef372 | 0;
  D: number = 0xa54ff53a | 0;
}

function rotr(w: number, s: number): number {
  return (w << (32 - s)) | (w >>> s);
}

// noble SHA2_32B.process.
function rounds(st: any, n: number): number {
  let { A, B, C, D } = st;
  for (let i = 0; i < n; i++) {
    const t = (rotr(A, 7) ^ ((B & C) ^ (~B & D))) + i | 0;
    D = C; C = B; B = A; A = t;
  }
  st.A = A; st.B = B; st.C = C; st.D = D;
  return A;
}

// Every special ToInt32 input, entering as the loop's local.
function mix(seed: any, n: number): string {
  let h = seed;
  let out = "";
  for (let i = 0; i < n; i++) {
    h = (h ^ (h << 5)) + ~h;
    out += String(h) + ",";
  }
  return out + String(h);
}

// The local is only ever read, never written, before the first write: the
// entry value flows through unchanged when the loop runs zero times.
function zeroTrip(seed: any): any {
  let h = seed;
  for (let i = 0; i < 0; i++) h = ~h & 7;
  return h;
}

// A write in the UPDATE clause that is not a Number withdraws the local.
function updateWrite(n: number): string {
  let s: any = 1;
  let acc = 0;
  for (let i = 0; i < n; i++, s = "a") {
    acc = acc + (~s | 0);
  }
  return acc + "|" + s;
}

// A write in the CONDITION clause that is not a Number withdraws it too.
function conditionWrite(n: number): string {
  let s: any = 3;
  let acc = 0;
  let i = 0;
  for (; (s = i < 2 ? s : "x"), i < n; i++) {
    acc = acc ^ ~s;
  }
  return acc + "|" + s;
}

// Early exits: the caller observes the local after a break and a return.
function breakOut(seed: number, n: number): number {
  let h: number = seed;
  for (let i = 0; i < n; i++) {
    h = (h * 31 + i) & 0xffff;
    if ((h & 3) === 3) break;
    h = ~h ^ 0x55;
  }
  return h;
}
function returnOut(seed: number, n: number): number {
  let h: number = seed;
  for (let i = 0; i < n; i++) {
    h = (h << 1) ^ i;
    if (h > 1000) return h + 0.5;
  }
  return -h;
}

// A throw from inside the loop, caught by the caller.
function throwOut(seed: number, n: number): number {
  let h: number = seed;
  for (let i = 0; i < n; i++) {
    h = ~h ^ (i << 3);
    if (i === 5) throw new Error("h=" + h);
  }
  return h;
}

// A throw caught in the SAME function after the loop: the catch must see the
// value the loop wrote, not the entry value.
function throwCaught(seed: number): number {
  let h: number = seed;
  try {
    for (let i = 0; i < 10; i++) {
      h = (h ^ 0x3c) + 1 | 0;
      if (i === 3) throw new Error("x");
    }
  } catch (e) {
    return h;
  }
  return -1;
}

// Nested loops: the inner loop writes the outer loop's local.
function nested(seed: number): number {
  let h: number = seed;
  for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 3; j++) {
      h = (h >>> 1) ^ (~h & j);
    }
    h = h ^ i;
  }
  return h;
}

const show = (label: string, v: unknown) => console.log(label, String(v));

const st = new St();
show("rounds", rounds(st, 64));
show("roundsState", [st.A, st.B, st.C, st.D].join(","));
show("roundsZero", rounds({ A: 1.5, B: -0, C: NaN, D: "7" }, 0));

for (const seed of [0, -0, 1, -1, 1.5, -1.5, NaN, Infinity, -Infinity, 2 ** 31, 2 ** 32 + 5, -(2 ** 53), 1e300, "12", " 0x10 ", "abc", "", true, null, undefined, [3], { valueOf: () => 9 }]) {
  show("mix " + (typeof seed === "string" ? JSON.stringify(seed) : Object.is(seed, -0) ? "-0" : String(seed)), mix(seed, 3));
}
show("zeroTrip -0", Object.is(zeroTrip(-0), -0));
show("zeroTrip str", zeroTrip("s"));
show("updateWrite", updateWrite(3));
show("conditionWrite", conditionWrite(4));
show("breakOut", breakOut(7, 100));
show("returnOut", returnOut(3, 100));
try {
  throwOut(11, 10);
} catch (e) {
  show("throwOut", (e as Error).message);
}
show("throwCaught", throwCaught(5));
show("nested", nested(0x12345));

// BigInt operands: the entry test fails and the ordinary loop throws exactly
// where node does.
try {
  show("bigint", mix(5n, 2));
} catch (e) {
  show("bigint", (e as Error).constructor.name);
}
try {
  rounds({ A: 1n, B: 2n, C: 3n, D: 4n }, 2);
  show("bigintRounds", "no throw");
} catch (e) {
  show("bigintRounds", (e as Error).constructor.name);
}
// BigInt-only arithmetic stays BigInt in the ordinary clone.
function bigOnly(seed: bigint, n: number): bigint {
  let h = seed;
  for (let i = 0; i < n; i++) h = (h ^ (h << 3n)) & 0xffffn;
  return h;
}
show("bigOnly", bigOnly(7n, 5));
