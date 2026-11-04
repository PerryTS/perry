// A receiver annotated with a utility type (`Pick`, `Omit`, `Readonly`) calls
// its own `join`/`find`, not the Array fast path those names share; genuine
// array receivers keep it.
interface Svc {
  readonly sep: string;
  readonly join: (...s: ReadonlyArray<string>) => string;
  readonly find: (n: string) => string;
}
const svc: Svc = {
  sep: "/",
  join: (...s) => s.join("/"),
  find: (n) => `found:${n}`,
};

function viaPick(p: Pick<Svc, "join">, root: string): string {
  return p.join(root, "supabase", ".temp");
}
function viaOmit(p: Omit<Svc, "sep">, root: string): string {
  return p.join(root, "x");
}
function viaReadonly(p: Readonly<Svc>, root: string): string {
  return p.join(root, "y");
}
function viaPartial(p: Partial<Svc>, root: string): string {
  return p.join!(root, "z");
}
function viaPickFind(p: Pick<Svc, "find">): string {
  return p.find("needle");
}
function viaReadonlyArray(a: ReadonlyArray<string>): string {
  return a.join("-");
}
function viaArray(a: Array<string>): string {
  return a.join("-") + ":" + a.indexOf("b") + ":" + a.find((s) => s > "a");
}

console.log(viaPick(svc, "/w"));
console.log(viaOmit(svc, "/w"));
console.log(viaReadonly(svc, "/w"));
console.log(viaPartial(svc, "/w"));
console.log(viaPickFind(svc));
console.log(viaReadonlyArray(["a", "b", "c"]));
console.log(viaArray(["a", "b", "c"]));
