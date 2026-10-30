// Small Map lookups (<= 8 entries) at every slot, over deletions, and with the
// keys a bit-identity scan must not decide on its own: -0, NaN, strings equal
// only by content, objects by identity.
function show(label: string, v: unknown): void {
  console.log(label, String(v));
}

for (let n = 1; n <= 9; n++) {
  const m = new Map<number, number>();
  for (let k = 0; k < n; k++) m.set(100 + k, k * 3);
  const got: number[] = [];
  for (let k = n - 1; k >= 0; k--) got.push(m.get(100 + k) as number);
  show("n=" + n + " reverse", got.join(","));
  show("n=" + n + " miss", [m.has(99), m.has(100 + n), m.get(7)].join(","));
  for (let k = 0; k < n; k++) {
    m.delete(100 + k);
    const rest: string[] = [];
    for (let r = 0; r < n; r++) rest.push(m.has(100 + r) ? "1" : "0");
    show("n=" + n + " del" + k, rest.join(""));
  }
  show("n=" + n + " size-after", m.size);
}

const z = new Map<number, string>();
z.set(0, "zero");
z.set(NaN, "nan");
z.set(5, "five");
show("-0", z.get(-0));
show("+0", z.get(0));
show("NaN", z.get(0 / 0));
show("five", z.get(5));
z.set(-0, "negzero");
show("zero-size", z.size + ":" + z.get(0));

const s = new Map<string, number>();
const k1 = "a" + "b";
const k2 = ["a", "b"].join("");
s.set(k1, 1);
s.set("cd", 2);
s.set("ef", 3);
show("str-content", [s.get(k2), s.get("ab"), s.get("cd"), s.get("ef"), s.get("zz")].join(","));
s.delete("cd");
show("str-after-delete", [s.get(k2), s.get("cd"), s.get("ef"), s.size].join(","));
s.set("cd", 9);
show("str-reinsert", [...s.entries()].map((e) => e.join("=")).join(";"));

const o1 = {};
const o2 = {};
const om = new Map<object, string>();
om.set(o1, "one");
om.set(o2, "two");
show("obj", [om.get(o1), om.get({}), om.get(o2)].join(","));

const mixed = new Map<unknown, number>();
mixed.set(1, 1);
mixed.set("1", 2);
mixed.set(true, 3);
mixed.set(null, 4);
mixed.set(undefined, 5);
mixed.set(1n, 6);
show("mixed", [mixed.get(1), mixed.get("1"), mixed.get(true), mixed.get(null), mixed.get(undefined), mixed.get(1n), mixed.get(2n), mixed.get(false)].join(","));
