import * as url from "node:url";

function show(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label + ":", String(value));
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code", err?.message);
  }
}

show("parse number", () => url.parse(123 as any));
show("parse null", () => url.parse(null as any));

const parsedTruthy = url.parse("//example.com/a?x=1", "yes" as any, "no" as any);
console.log("parse truthy host:", parsedTruthy.host);
console.log("parse truthy protocol:", parsedTruthy.protocol === null ? "null" : parsedTruthy.protocol);
console.log("parse truthy query x:", (parsedTruthy.query as any).x);

show("resolve from-number", () => url.resolve(123 as any, "/x"));
show("resolve to-number", () => url.resolve("http://example.com/a", 123 as any));
show("resolve from-symbol", () => url.resolve(Symbol("x") as any, "/x"));
console.log("resolve valid:", url.resolve("http://example.com/a/b", "../c"));
