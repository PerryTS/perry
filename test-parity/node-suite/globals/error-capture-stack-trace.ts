function show(label: string, fn: () => unknown) {
  try {
    console.log(label, fn());
  } catch (err: any) {
    console.log(label, "throw", err?.name);
  }
}

const obj: any = {};
console.log("typeof:", typeof Error.captureStackTrace);
Error.captureStackTrace(obj);
console.log("stack string:", typeof obj.stack === "string");
console.log("direct non-enum:", obj.propertyIsEnumerable("stack"));
console.log("keys hidden:", Object.keys(obj).includes("stack"));
console.log("undefined enum:", ({ x: undefined } as any).propertyIsEnumerable("x"));

function factory() {
  const out: any = {};
  Error.captureStackTrace(out, factory);
  return String(out.stack).includes("factory");
}
console.log("filtered:", factory());

show("null target:", () => Error.captureStackTrace(null as any));
show("number target:", () => Error.captureStackTrace(1 as any));
