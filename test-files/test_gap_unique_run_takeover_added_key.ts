// A receiver whose own key list grows unique to it hands its keys to its
// overflow once the run reaches 1024 keys. The takeover runs while the key
// that reached it is being added, before that key's value is stored: the key
// must stay an own, enumerable key (it was dropped from Object.keys when the
// object also had an accessor and the value was a function).
function run(n: number, mode: string) {
  const o: any = {};
  Object.defineProperty(o, "first", { get() { return "G"; }, enumerable: true, configurable: true });
  for (let i = 0; i < n; i++) {
    o["g" + i] = mode === "fn" ? ((v: number) => v - i) : mode === "obj" ? { i } : mode === "str" ? "s" + i : i;
  }
  const ks = Object.keys(o);
  const missing: string[] = [];
  for (let i = 0; i < n; i++) if (ks.indexOf("g" + i) < 0) missing.push("g" + i);
  const last = "g" + (n - 1);
  console.log(mode, n, ks.length, missing.join(","), typeof o[last], last in o,
    Object.prototype.hasOwnProperty.call(o, last), Object.getOwnPropertyNames(o).length, ks[ks.length - 1]);
}
for (const mode of ["fn", "obj", "str", "num"]) {
  for (const n of [255, 1022, 1023, 1024, 2047]) run(n, mode);
}
function plain(n: number) {
  const o: any = {};
  for (let i = 0; i < n; i++) o["p" + i] = () => i;
  const ks = Object.keys(o);
  console.log("plain", n, ks.length, ks[ks.length - 1], o["p" + (n - 1)]());
}
plain(1024);
plain(1025);
