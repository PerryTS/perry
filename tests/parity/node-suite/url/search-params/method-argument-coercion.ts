function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (err: any) {
    console.log(
      label,
      "THROW",
      err?.name || "no-name",
      err?.code || "no-code",
      String(err?.message).split("\n")[0],
    );
  }
}

let sp: URLSearchParams;
let missingArgTicks = 0;
let valueTicks = 0;

function sideName() {
  missingArgTicks += 1;
  return "a";
}

function sideValue() {
  valueTicks += 1;
  return "v";
}

probe("append number null", () => {
  sp = new URLSearchParams();
  sp.append(123 as any, null as any);
  return sp.toString();
});

probe("set object bool", () => {
  sp = new URLSearchParams();
  sp.set({ toString() { return "k"; } } as any, true as any);
  return sp.toString();
});

probe("get number null", () => {
  sp = new URLSearchParams("123=x&null=y");
  return `${sp.get(123 as any)},${sp.get(null as any)}`;
});

probe("getAll bool", () => {
  sp = new URLSearchParams("true=a&true=b");
  return sp.getAll(true as any).join(",");
});

probe("has value number", () => {
  sp = new URLSearchParams("a=1&a=2");
  return `${sp.has("a", 2 as any)},${sp.has("a", 3 as any)}`;
});

probe("delete value number", () => {
  sp = new URLSearchParams("a=1&a=2&a=3");
  sp.delete("a", 2 as any);
  return sp.toString();
});

probe("append symbol", () => {
  sp = new URLSearchParams();
  sp.append(Symbol("x") as any, "v");
  return sp.toString();
});

probe("append symbol evals value", () => {
  valueTicks = 0;
  sp = new URLSearchParams();
  try {
    sp.append(Symbol("x") as any, sideValue() as any);
  } catch (err: any) {
    return `${valueTicks}:${err?.name || "no-name"}:${err?.code || "no-code"}:${String(err?.message).split("\n")[0]}`;
  }
  return "no throw";
});

probe("append missing value", () => {
  sp = new URLSearchParams();
  sp.append("a" as any);
  return sp.toString();
});

probe("append missing value eval", () => {
  missingArgTicks = 0;
  sp = new URLSearchParams();
  try {
    sp.append(sideName() as any);
  } catch (err: any) {
    return `${missingArgTicks}:${err?.code || "no-code"}:${String(err?.message).split("\n")[0]}`;
  }
  return "no throw";
});

probe("set missing value", () => {
  sp = new URLSearchParams();
  sp.set("a" as any);
  return sp.toString();
});

probe("get missing name", () => {
  sp = new URLSearchParams("a=1");
  return sp.get();
});
