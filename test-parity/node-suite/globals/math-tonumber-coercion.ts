function show(label: string, value: number): void {
  if (Object.is(value, -0)) {
    console.log(label, "-0");
  } else if (Number.isNaN(value)) {
    console.log(label, "NaN");
  } else {
    console.log(label, String(value));
  }
}

function throwsTypeError(label: string, thunk: () => unknown): void {
  try {
    console.log(label, "value", thunk());
  } catch (err: any) {
    console.log(label, "throw", err instanceof TypeError, err.name);
  }
}

const sym = Symbol("math");

throwsTypeError("abs symbol", () => Math.abs(sym as any));
throwsTypeError("sin bigint", () => Math.sin(5n as any));
throwsTypeError("pow symbol", () => Math.pow(sym as any, 2));
throwsTypeError("atan2 bigint", () => Math.atan2(1, 5n as any));
throwsTypeError("fround symbol", () => Math.fround(sym as any));
throwsTypeError("clz32 bigint", () => Math.clz32(5n as any));
throwsTypeError("hypot symbol", () => Math.hypot(3, sym as any));
throwsTypeError("min symbol", () => Math.min(1, sym as any));
throwsTypeError("max bigint", () => Math.max(1, 5n as any));

show("abs string", Math.abs("-5" as any));
show("floor bool", Math.floor(true as any));
show("ceil null", Math.ceil(null as any));
show("sqrt string", Math.sqrt("16" as any));
show("sin string", Math.sin("0" as any));
show("pow strings", Math.pow("2" as any, "3" as any));
show("imul strings", Math.imul("2" as any, "3" as any));
show("fround true", Math.fround(true as any));
show("clz32 string", Math.clz32("1" as any));
show("hypot mixed", Math.hypot("3" as any, true as any));
show("min mixed", Math.min("3" as any, true as any));
show("max mixed", Math.max("3" as any, true as any));
