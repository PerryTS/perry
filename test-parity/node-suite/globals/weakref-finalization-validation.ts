function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(label + ":", err?.name);
  }
}

const weakTarget = {};
const weakRef = new WeakRef(weakTarget);
show("weakref object", () => weakRef.deref() === weakTarget);
show("weakref primitive", () => new WeakRef(1 as any));
show("weakref no arg", () => new (WeakRef as any)());

show("fr callback", () => !!new FinalizationRegistry(() => {}));
show("fr callback primitive", () => new FinalizationRegistry(1 as any));
show("fr no callback", () => new (FinalizationRegistry as any)());

const fr = new FinalizationRegistry(() => {});
show("fr primitive target", () => fr.register(1 as any, "held"));

const obj = {};
show("fr held same target", () => fr.register(obj, obj));
show("fr primitive register token", () => fr.register(obj, "held", 1 as any));
show("fr primitive token", () => fr.unregister(1 as any));

const token = {};
fr.register(obj, "held", token);
show("fr object token", () => fr.unregister(token));
