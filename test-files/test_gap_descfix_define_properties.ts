// Object.defineProperties applies every collected definition in order and
// stops at the first rejected one: earlier definitions stay, later ones are not
// applied. Exercised on an ordinary object and on a native handle target.
function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, String(f()));
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError ? "TypeError" : String(e));
  }
}

const targets: Array<[string, () => any]> = [
  ["plain", () => ({})],
  ["headers", () => new Headers()],
  ["response", () => new Response("x")],
];

for (const [name, make] of targets) {
  const t = make();
  attempt(name + " all accepted", () => {
    Object.defineProperties(t, {
      a: { value: 1, enumerable: true },
      b: { get() { return 2; }, enumerable: true },
      [Symbol.for("descfix")]: { value: 3 },
    });
    return t.a + "," + t.b + "," + t[Symbol.for("descfix")];
  });
  attempt(name + " fails midway", () => {
    Object.defineProperties(t, {
      c: { value: "c", configurable: true },
      a: { value: 99 },
      d: { value: "d" },
    });
    return "no throw";
  });
  attempt(name + " state", () => [t.a, t.c, t.d, "d" in t].join(","));
  attempt(name + " create", () => {
    const o = Object.create(null, { e: { value: 5, enumerable: true }, f: { get() { return 6; } } });
    return o.e + "," + o.f + "," + Object.keys(o).join("|");
  });
  attempt(name + " bad descriptor first", () => {
    const fresh = make();
    Object.defineProperties(fresh, { g: { value: 1 }, h: 5 as any });
    return "no throw";
  });
  attempt(name + " bad descriptor applied nothing", () => {
    const fresh = make();
    try { Object.defineProperties(fresh, { g: { value: 1 }, h: 5 as any }); } catch {}
    return "g" in fresh;
  });
}
