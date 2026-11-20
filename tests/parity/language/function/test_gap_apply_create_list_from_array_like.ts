// Function.prototype.apply is CreateListFromArrayLike (ECMA-262 20.2.3.1), not iteration:
// an array-like object is accepted (length + indexed reads), undefined/null pass no
// arguments, and a non-object list throws TypeError. Spread (GetIterator) is a different op.
function f(...a: any[]) { return a.length + ":" + a.join(","); }
function g(a: any, b: any) { return a + "," + b; }
class V { set(...a: any[]) { return a.length + ":" + a.join(","); } fixed(a: any, b: any) { return a + "/" + b; } }
const v = new V();
function t(label: string, fn: () => any) { try { console.log(label, fn()); } catch (e: any) { console.log(label, "throws", e.constructor.name); } }
const al: any = { length: 2, 0: 65, 1: 66 };
t("fromCharCode-arraylike", () => String.fromCharCode.apply(null, al));
t("fromCharCode-undef", () => String.fromCharCode.apply(null, undefined as any));
t("fromCharCode-num", () => String.fromCharCode.apply(null, 5 as any));
t("rest-arraylike", () => f.apply(null, al));
t("fixed-arraylike", () => g.apply(null, al));
t("fixed-short-arraylike", () => g.apply(null, { length: 1, 0: "x" } as any));
t("rest-undef", () => f.apply(null, undefined as any));
t("rest-null", () => f.apply(null, null as any));
t("fixed-null", () => g.apply(null, null as any));
t("rest-num", () => f.apply(null, 7 as any));
t("rest-string", () => f.apply(null, "ab" as any));
t("m-arraylike", () => v.set.apply(v, al));
t("m-fixed-arraylike", () => v.fixed.apply(v, { length: 2, 0: "p", 1: "q" } as any));
t("m-undef", () => v.set.apply(v, undefined as any));
t("max-arraylike", () => Math.max.apply(null, { length: 3, 0: 1, 1: 9, 2: 4 } as any));
t("max-null", () => Math.max.apply(null, null as any));
t("push-arraylike", () => { const a: any[] = [0]; Array.prototype.push.apply(a, al); return a.join(","); });
function viaArguments(..._x: any[]) { return f.apply(null, arguments as any); }
t("arguments", () => viaArguments(1, 2, 3));
function fixedArgs(a: any, b: any) { return g.apply(null, arguments as any); }
t("arguments-fixed", () => fixedArgs("u", "w"));
t("holes", () => f.apply(null, { length: 3, 1: "m" } as any));
t("big-length", () => f.apply(null, { length: "2", 0: "s", 1: "t" } as any));
t("reflect-arraylike", () => Reflect.apply(f, null, al));
t("reflect-undef", () => Reflect.apply(f, null, undefined as any));
