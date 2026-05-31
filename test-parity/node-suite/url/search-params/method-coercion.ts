function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(label + " err:", err?.name, err?.code || "no-code", err?.message);
  }
}

const params = new URLSearchParams("false=bool&null=nil&undefined=undef&obj=object&k=1&k=2");

show("get bool", () => params.get(false as any));
show("get null", () => params.get(null as any));
show("get undefined", () => params.get(undefined as any));
show("get object", () => params.get({ toString() { return "obj"; } } as any));
show("getAll object", () => params.getAll({ toString() { return "k"; } } as any).join(","));
show("has value number", () => params.has("k", 2 as any));
show("delete value number", () => {
  const copy = new URLSearchParams("k=1&k=2&k=3");
  copy.delete("k", 2 as any);
  return copy.toString();
});
show("set append mixed", () => {
  const copy = new URLSearchParams();
  copy.set(null as any, undefined as any);
  copy.append(false as any, { toString() { return "obj"; } } as any);
  return copy.toString();
});
show("get symbol", () => params.get(Symbol("x") as any));
show("append symbol", () => {
  const copy = new URLSearchParams();
  copy.append("x", Symbol("y") as any);
  return copy.toString();
});
