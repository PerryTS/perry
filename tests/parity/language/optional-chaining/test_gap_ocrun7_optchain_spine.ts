// A non-optional link continuing an optional chain of several ?. links
// (o?.s?.x().y) runs inside the whole short-circuit spine: a nullish
// link short-circuits the rest, as in node.
const o: any = { s: undefined };
const t = (name: string, f: () => any) => { try { console.log(name, JSON.stringify(f())); } catch (e: any) { console.log(name, "THROW", e.message); } };
t("k", () => o?.s?.x.y);
t("l", () => o?.s?.trim());
t("m", () => o?.s?.x().y);
t("n", () => o?.s?.x.y());
t("p", () => o?.s.x);
t("q", () => o?.s?.x?.y.z);
t("r", () => o?.q?.r?.trim().toLowerCase());
t("s", () => o.s?.x?.trim().toLowerCase());
t("u", () => o?.s?.x().y());
t("v", () => o?.s?.x()());
t("w", () => o?.s?.x()[0]);
const z: any = { s: { x() { return { y: 7, f() { return 8; } }; } } };
t("x1", () => z?.s?.x().y);
t("x2", () => z?.s?.x().f());
let calls = 0; const c: any = { get s() { calls++; return { x() { return { y: 1 }; } }; } };
t("x3", () => [c?.s?.x().y, calls]);
t("x4", () => (o?.s ? undefined : { y: 3 })?.y);
// The @opentui/keymap command query shape.
const query = (options: any) => options.query?.search?.trim().toLowerCase() ?? "";
t("keymap", () => [query({ query: { namespace: "palette" } }), query({ query: { search: " AbC " } }), query({})]);
