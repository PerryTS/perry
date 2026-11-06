function show(label: string, fn: () => any) {
  try {
    const returned = fn();
    console.log(label, "OK", returned === process, String(process.getMaxListeners()));
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code);
  }
}

show("set zero", () => process.setMaxListeners(0));
show("set one", () => process.setMaxListeners(1));
show("set fraction", () => process.setMaxListeners(1.5));
show("set infinity", () => process.setMaxListeners(Infinity));
show("set negative", () => process.setMaxListeners(-1));
show("set nan", () => process.setMaxListeners(NaN));
show("set string", () => process.setMaxListeners("5" as any));
show("set null", () => process.setMaxListeners(null as any));
show("set undefined", () => process.setMaxListeners(undefined as any));
show("set boolean", () => process.setMaxListeners(true as any));
