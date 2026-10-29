// Map get-then-set with the builtin get/set replaced (subclass override, own property):
// the replacement runs, never the fused slot store.
const dump = (label: string, m: Map<any, any>): void => {
  const parts: string[] = [];
  m.forEach((v: any, k: any) => {
    parts.push((typeof k === "object" ? "obj" : Object.is(k, -0) ? "-0" : String(k)) + "=" + (typeof v === "string" ? v.slice(0, 12) + "#" + v.length : typeof v === "object" ? "obj" : String(v)));
  });
  console.log(label + " size=" + m.size + " [" + parts.join(",") + "]");
};
const churn = (n: number): number => {
  let s = "";
  for (let i = 0; i < n; i++) s += "x" + i;
  return s.length;
};

// 13. subclass overriding get and set
{
  class Counting extends Map<any, any> {
    gets = 0;
    sets = 0;
    get(k: any) { this.gets++; return super.get(k); }
    set(k: any, v: any) { this.sets++; return super.set(k, v + 1000); }
  }
  const m = new Counting();
  for (let i = 0; i < 3; i++) m.set("a", ((m.get("a") as any) ?? 0) + 1);
  console.log("subclass", m.gets, m.sets, m.get("a"));
}
// 14. own-property override of get on a Map instance
{
  const m = new Map<any, any>([["a", 1]]);
  (m as any).get = (k: any) => 41;
  m.set("a", ((m as any).get("a") as any) + 1);
  console.log("own-get", Map.prototype.get.call(m, "a"));
}
