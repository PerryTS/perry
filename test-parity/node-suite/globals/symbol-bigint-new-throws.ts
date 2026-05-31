function describe(value: unknown): string {
  if (typeof value === "symbol") {
    return `symbol:${String(value)}`;
  }
  if (typeof value === "bigint") {
    return `bigint:${value.toString()}`;
  }
  const ctor = (value as any)?.constructor?.name ?? "no-ctor";
  return `${typeof value}:${ctor}`;
}

function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "ok", describe(fn()));
  } catch (err: any) {
    console.log(label, err?.name, err?.message);
  }
}

const SymbolAlias: any = Symbol;
const BigIntAlias: any = BigInt;

show("symbol call", () => Symbol("x"));
show("bigint call", () => BigInt("42"));

show("symbol direct", () => new (Symbol as any)("x"));
show("bigint direct", () => new (BigInt as any)("1"));
show("symbol global", () => new (globalThis.Symbol as any)("x"));
show("bigint global", () => new (globalThis.BigInt as any)("1"));
show("symbol alias", () => new SymbolAlias("x"));
show("bigint alias", () => new BigIntAlias("1"));
