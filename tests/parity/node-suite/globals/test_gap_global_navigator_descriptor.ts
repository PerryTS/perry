// globalThis.navigator is a lazy accessor of the global object, as in node:
// an own accessor (get only), enumerable and configurable; the getter builds
// one object, and replacing or deleting the property is seen by later reads.
const g: any = globalThis;
const out: string[] = [];
const d = Object.getOwnPropertyDescriptor(g, "navigator")!;
out.push(
  [typeof d.get, typeof d.set, "value" in d, d.enumerable, d.configurable].join(","),
);
out.push(String(g.navigator === g.navigator) + "," + typeof navigator);
out.push(String(Object.prototype.hasOwnProperty.call(g, "navigator")));
for (const n of ["crypto", "localStorage", "sessionStorage"]) {
  const a = Object.getOwnPropertyDescriptor(g, n);
  out.push(n + ":" + (a ? [typeof a.get, a.enumerable, a.configurable].join(",") : "none"));
}
Object.defineProperty(g, "navigator", { value: "replaced", configurable: true });
out.push(String(g.navigator));
delete g.navigator;
out.push(typeof g.navigator + "," + ("navigator" in g));
console.log(out.join("\n"));
