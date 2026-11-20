// navigator's members are Navigator.prototype getters, as in node: the
// navigator object has no own properties, each getter is enumerable and
// configurable with no setter, values are kept per instance (languages and
// locks are the same object on every read, languages is frozen), and a
// getter called on another receiver throws a TypeError.
const P: any = (globalThis as any).Navigator.prototype;
const nav: any = (globalThis as any).navigator;
const out: string[] = [];
out.push("own: " + JSON.stringify(Object.getOwnPropertyNames(nav)));
out.push("proto keys: " + JSON.stringify(Object.getOwnPropertyNames(P)));
out.push("proto is: " + String(Object.getPrototypeOf(nav) === P));
for (const k of ["userAgent", "language", "languages", "hardwareConcurrency", "platform", "locks"]) {
  const d = Object.getOwnPropertyDescriptor(P, k)!;
  out.push(
    k + ": " +
      [typeof d.get, typeof d.set, "value" in d, d.enumerable, d.configurable,
        d.get!.name, d.get!.length, typeof nav[k], k in nav,
        Object.prototype.hasOwnProperty.call(nav, k)].join(","),
  );
  let threw = "none";
  try {
    d.get!.call({});
  } catch (e: any) {
    threw = e instanceof TypeError ? "TypeError" : String(e);
  }
  out.push(k + " on {}: " + threw);
}
out.push("languages same: " + String(nav.languages === nav.languages));
out.push("languages frozen: " + String(Object.isFrozen(nav.languages)));
out.push("languages array: " + String(Array.isArray(nav.languages) && nav.languages.length === 1));
out.push("language is languages[0]: " + String(nav.language === nav.languages[0]));
out.push("locks same: " + String(nav.locks === nav.locks) + "," + typeof nav.locks);
out.push("cores: " + String(Number.isInteger(nav.hardwareConcurrency) && nav.hardwareConcurrency >= 1));
out.push("userAgent: " + String(nav.userAgent.startsWith("Node.js/")));
out.push("keys: " + JSON.stringify(Object.keys(nav)));
const seen: string[] = [];
for (const k in nav) seen.push(k);
out.push("for-in: " + JSON.stringify(seen));
console.log(out.join("\n"));
