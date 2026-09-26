// Gap test (#10697): string-keyed Map lookups through the decoded-probe lane.
// A key reaches the map as a short (inline) string, a literal, a slice, a
// concatenation or a template, so the same content arrives in different
// encodings. Every lookup must match on content alone, on a small map
// (linear scan) and on one past the index threshold, next to symbol, number
// and object keys that must never collide with a string.
// Run: node --experimental-strip-types test_gap_10697_map_string_key_probe.ts

const CATS = ["alpha", "beta", "gamma", "delta"];

function countByCategory(n: number): Map<string, number> {
  const m = new Map<string, number>();
  for (let i = 0; i < n; i++) {
    const k = CATS[i & 3];
    m.set(k, (m.get(k) ?? 0) + 1);
  }
  return m;
}
const counts = countByCategory(1001);
console.log("counts:", [...counts.entries()].join(" "));

// Same content, different construction: literal, slice, concat, template, join.
const built = [
  "alpha",
  "xalphax".slice(1, 6),
  "al" + "pha",
  `${"alp"}${"ha"}`,
  ["a", "l", "p", "h", "a"].join(""),
  "category_long",
  "category_" + "long",
  `category_${"lo"}ng`,
  "",
  "x".slice(1),
];
for (const k of built) {
  console.log(JSON.stringify(k), counts.get(k), counts.has(k));
}

function probe(label: string, m: Map<unknown, unknown>, keys: unknown[]): void {
  const out: string[] = [];
  for (const k of keys) out.push(String(m.get(k)) + "/" + (m.has(k) ? "y" : "n"));
  console.log(label, out.join(" "));
}

const sym = Symbol();
const obj = {};
for (const pad of [0, 20]) {
  const m = new Map<unknown, unknown>();
  m.set(sym, "sym");
  m.set("beta", "b");
  m.set("category_long", "long");
  m.set(42, "num");
  m.set(obj, "obj");
  for (let i = 0; i < pad; i++) m.set("pad_" + i, i);
  m.set("", "empty");
  const keys: unknown[] = [
    "", "x".slice(1), "beta", "be" + "ta", "betA", "bet", "betaa",
    "category_long", "category_" + "long", "category_lonG",
    "pad_7", "pad_" + 7, "pad_99", "42", 42, sym, Symbol(), obj, {},
  ];
  probe("pad=" + pad, m, keys);
  // Delete and re-insert through a differently-built key.
  console.log("delete", m.delete("be" + "ta"), m.has("beta"), m.size);
  m.set(`${"be"}ta`, "b2");
  console.log("reinsert", m.get("beta"), m.size, [...m.keys()].slice(-1)[0]);
}

// Non-ASCII content of short and long byte length.
const u = new Map<string, number>();
u.set("é", 1);
u.set("日本", 2);
u.set("naïve-string", 3);
console.log(u.get("é".normalize("NFC")), u.get("日" + "本"), u.get("naïve-" + "string"), u.get("é"));
