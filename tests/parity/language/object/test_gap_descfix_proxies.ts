// Proxies forwarding [[DefineOwnProperty]] to a native handle or ordinary
// target: without a trap the target's own verdict is used; with a trap the
// trap's result is checked against the target's invariants.
function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, String(f()));
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError ? "TypeError" : String(e));
  }
}

const S = Symbol("descfix-proxy");
const targets: Array<[string, () => any]> = [
  ["plain", () => ({})],
  ["headers", () => new Headers()],
  ["request", () => new Request("http://localhost/p")],
];

for (const [name, make] of targets) {
  attempt(name + " no trap", () => {
    const target = make();
    const p = new Proxy(target, {});
    Object.defineProperty(p, "a", { value: 1, configurable: true });
    Object.defineProperty(p, S, { value: 2 });
    return target.a + "," + target[S] + "," + Reflect.defineProperty(p, "b", { value: 3 });
  });
  attempt(name + " no trap reject", () => {
    const target = make();
    Object.defineProperty(target, "fixed", { value: 1 });
    const p = new Proxy(target, {});
    return Reflect.defineProperty(p, "fixed", { value: 2 });
  });
  attempt(name + " forwarding trap", () => {
    const target = make();
    const seen: string[] = [];
    const p = new Proxy(target, {
      defineProperty(t, key, desc) {
        seen.push(String(key) + ":" + Object.keys(desc).join("|"));
        return Reflect.defineProperty(t, key, desc);
      },
    });
    Object.defineProperty(p, "c", { value: 3, enumerable: true });
    const r = Reflect.defineProperty(p, "c", { value: 4 });
    return target.c + "," + r + "," + seen.join(";");
  });
  attempt(name + " lying trap", () => {
    const target = make();
    const p = new Proxy(target, { defineProperty() { return true; } });
    // Reporting success for a non-configurable definition the target lacks
    // violates the proxy invariant.
    Object.defineProperty(p, "d", { value: 1, configurable: false });
    return "no throw";
  });
  attempt(name + " falsish trap", () => {
    const p = new Proxy(make(), { defineProperty() { return false; } });
    const r = Reflect.defineProperty(p, "e", { value: 1 });
    Object.defineProperty(p, "e", { value: 1 });
    return r + " no throw";
  });
}
