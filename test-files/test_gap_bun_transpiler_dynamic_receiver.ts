// A Bun.Transpiler reached through an untyped value (any local, object field,
// Map value). Its handle id was claimed by no dynamic dispatch, so every such
// method call silently returned undefined.
const t = new Bun.Transpiler({ loader: "ts" });
const anyT: any = t;
console.log("any", JSON.stringify(anyT.transformSync("const a: number = 1;")));
const holder: any = { t };
console.log("field", JSON.stringify(holder.t.transformSync("let b: string = \"x\";")));
const m = new Map<string, unknown>([["t", t]]);
const fromMap = m.get("t") as any;
console.log("loader", JSON.stringify(fromMap.transformSync("const c = <div/>;", "jsx").length > 0));
console.log("imports", anyT.scanImports("import x from \"./a\"; export const y = 1;").map((i: any) => i.path + ":" + i.kind).join());
console.log("scan", anyT.scan("import x from \"./a\"; export const y = 1;").exports.join());
const p = anyT.transform("const d: number = 4;");
console.log("promise", p instanceof Promise, JSON.stringify(await p));
console.log("typed", JSON.stringify(t.transformSync("const a: number = 1;")));
