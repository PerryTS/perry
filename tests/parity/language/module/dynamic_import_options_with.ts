// import(specifier, options) reads `options.with` once and enumerates it
// through [[OwnPropertyKeys]] before loading (EvaluateImportCall step 10).
const url = new URL("./fixtures/import_options/data.json", import.meta.url).href;

async function attempt(label: string, options: any): Promise<void> {
  try {
    const ns: any = await import(url, options);
    console.log(label, "loaded", ns.default.answer);
  } catch (error: any) {
    console.log(label, "rejected", error.name, error.message);
  }
}

let reads = 0;
await attempt("getter valid once:", {
  get with() {
    reads++;
    if (reads > 1) throw new Error("with read twice");
    return { type: "json" };
  },
});
console.log("with reads:", reads);

const trapped = new Proxy({ type: "json" }, {
  ownKeys() {
    throw new Error("ownKeys trap");
  },
});
await attempt("throwing ownKeys:", { with: trapped });

const traps: string[] = [];
const logged = new Proxy({ type: "json" }, {
  ownKeys(target) {
    traps.push("ownKeys");
    return Reflect.ownKeys(target);
  },
  getOwnPropertyDescriptor(target, key) {
    traps.push("gOPD:" + String(key));
    return Reflect.getOwnPropertyDescriptor(target, key);
  },
  get(target, key, receiver) {
    traps.push("get:" + String(key));
    return Reflect.get(target, key, receiver);
  },
});
await attempt("proxy attributes:", { with: logged });
console.log("traps:", traps.join(","));
