// Map and Set string keys: literal (atom) keys, computed keys with the same text,
// inline-short and long heap strings, across the small-map scan and the hashed index.
const lits = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
  "session.share", "session.compact", "a.rather.long.command.name.that.is.not.inline", "", "x", "naïve", "日本語"];
function built(s: string): string { return s.split("").join(""); }
for (const size of [3, 8, 9, 17]) {
  const keys = lits.slice(0, size);
  const m = new Map<string, number>();
  const s = new Set<string>();
  keys.forEach((k, i) => { m.set(k, i); s.add(k); });
  const hits: string[] = [];
  for (const k of keys) {
    const c = built(k);
    hits.push(`${JSON.stringify(k)}:${m.get(k)}/${m.get(c)}/${m.has(c)}/${s.has(c)}`);
  }
  // overwrite through a computed key, delete through a literal, re-add through a computed key
  if (keys.length > 2) {
    m.set(built(keys[1]), 100); m.delete(keys[2]); s.delete(built(keys[2]));
    m.set(built(keys[2]), 200); s.add(keys[2] + "");
  }
  console.log(size, m.size, s.size, hits.join(" "));
  console.log(size, JSON.stringify([...m]), JSON.stringify([...s]));
  console.log(size, m.get("missing"), m.has("alphaX"), s.has("bet"), m.get(built("missing")));
}
// keys that are property names of objects (minted atoms) used as Map keys
const o: Record<string, number> = { first: 1, second: 2, third: 3 };
const mm = new Map<string, number>();
for (let i = 0; i < 20; i++) mm.set("k" + i, i);
for (const k of Object.keys(o)) mm.set(k, o[k]);
console.log(mm.get("first"), mm.get(["sec", "ond"].join("")), mm.get("k13"), mm.has("k20"), mm.size);
const ss = new Set(Object.keys(o).concat(Array.from({ length: 12 }, (_, i) => `v${i}`)));
console.log(ss.has("third"), ss.has("v11"), ss.has("v12"), ss.size);
// trimmed literals (nothing to trim, and something to trim) as keys
const tm = new Map<string, number>();
for (let i = 0; i < 12; i++) tm.set(("cmd.number." + i).trim(), i);
const lit = ["app.exit", " app.help ", "theme.switch\n", "model.list"];
lit.forEach((k, i) => tm.set(k.trim(), 100 + i));
console.log(tm.get("app.exit"), tm.get("app.help"), tm.get("theme.switch"), tm.get(" app.help "), tm.size, "app.exit".trim() === "app.exit", " x ".trim().length);
