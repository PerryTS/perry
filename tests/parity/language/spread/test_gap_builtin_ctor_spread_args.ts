// A spread argument to a built-in constructor, an imported `Buffer` static,
// `BigInt` or `Symbol` must expand into separate arguments (#11838). perry
// folded the spread operand in as ONE argument holding the whole array.
import { Buffer } from "node:buffer";

function t(name: string, f: () => unknown) {
  try {
    console.log(name, f());
  } catch (e) {
    console.log(name, "threw", (e as Error).constructor.name);
  }
}

const ymd = [2020, 0, 2] as [number, number, number];
t("date", () => new Date(...ymd).getFullYear());
t("date-ms", () => new Date(...([86400000] as [number])).getTime());
t("date-mixed", () => new Date(2021, ...([1, 3] as [number, number])).getDate());
t("map", () => new Map(...([[[1, 2]]] as any)).get(1));
t("set", () => new Set(...([[1]] as any)).has(1));
t("set-size", () => new Set(...([[1, 2, 2]] as any)).size);
t("weakmap", () => new WeakMap(...([[[{}, 1]]] as any)) instanceof WeakMap);
t("weakset", () => new WeakSet(...([[{}]] as any)) instanceof WeakSet);
t("error", () => new Error(...(["boom"] as [string])).message);
t("typeerror", () => new TypeError(...(["tt"] as [string])).message);
t("error-cause", () => (new Error(...(["m", { cause: 7 }] as [string, any])) as any).cause);
t("array", () => JSON.stringify(new Array(...([1, 2, 3] as number[]))));
t("array-len", () => new Array(...([3] as [number])).length);
t("regexp", () => new RegExp(...(["a+", "g"] as [string, string])).flags);
t("number", () => new Number(...([5] as [number])).valueOf());
t("string", () => new String(...(["xy"] as [string])).length);
t("boolean", () => new Boolean(...([1] as [number])).valueOf());
t("object", () => (new Object(...([{ q: 1 }] as [any])) as any).q);
t("arraybuffer", () => new ArrayBuffer(...([8] as [number])).byteLength);
t("uint8", () => JSON.stringify(Array.from(new Uint8Array(...([[1, 2, 3]] as [number[]])))));
t("int32-len", () => new Int32Array(...([4] as [number])).length);
t("f64-buf", () => new Float64Array(...([new ArrayBuffer(16), 8, 1] as [ArrayBuffer, number, number])).length);
t("dataview", () => new DataView(...([new ArrayBuffer(16), 4] as [ArrayBuffer, number])).byteLength);
t("promise", () => typeof new Promise(...([(r: any) => r(1)] as [any])).then);
t("proxy", () => (new Proxy(...([{ a: 1 }, { get: () => 9 }] as [any, any])) as any).a);
t("url", () => new URL(...(["/p", "http://h.test"] as [string, string])).href);
t("urlsp", () => new URLSearchParams(...(["a=1&b=2"] as [string])).get("b"));
t("textdecoder", () => new TextDecoder(...(["utf-8"] as [string])).encoding);
t("aggregate", () => new AggregateError(...([[new Error("a")], "agg"] as [any, string])).message);
const wr = { w: 1 };
t("weakref", () => new WeakRef(...([wr] as [object])).deref() === wr);
t("event", () => new Event(...(["ping"] as [string])).type);
t("abortctl", () => new AbortController(...([] as [])).signal.aborted);
t("no-args-spread", () => new Date(...([] as [])) instanceof Date);

// Imported Buffer statics.
t("buf-concat", () => Buffer.concat(...([[Buffer.from("a"), Buffer.from("b")]] as [Buffer[]])).toString());
t("buf-from", () => Buffer.from(...(["hi", "utf8"] as [string, BufferEncoding])).toString("hex"));
t("buf-from-hex", () => Buffer.from(...(["6869", "hex"] as [string, BufferEncoding])).toString());
t("buf-alloc", () => Buffer.alloc(...([3, 1] as [number, number])).toString("hex"));
t("buf-isBuffer", () => Buffer.isBuffer(...([Buffer.from("x")] as [Buffer])));
t("buf-byteLength", () => Buffer.byteLength(...(["héllo", "utf8"] as [string, BufferEncoding])));
t("buf-isEncoding", () => Buffer.isEncoding(...(["utf8"] as [string])));
t("buf-allocUnsafe", () => Buffer.allocUnsafe(...([4] as [number])).length);
// Global Buffer (not imported).
t("gbuf-from", () => globalThis.Buffer.from(...(["hi", "utf8"] as [string, BufferEncoding])).toString("hex"));

// BigInt / Symbol.
t("bigint", () => BigInt(...([42] as [number])));
t("bigint-str", () => BigInt(...(["123"] as [string])));
t("symbol", () => Symbol(...(["desc"] as [string])).description);
t("symbol-for", () => Symbol.for(...(["k"] as [string])) === Symbol.for("k"));
t("bigint-asUintN", () => BigInt.asUintN(...([8, 257n] as [number, bigint])));

// More statics that folded the spread positionally.
t("date-utc", () => Date.UTC(...ymd));
t("date-parse", () => Date.parse(...(["2020-01-02T00:00:00Z"] as [string])));
t("arraybuffer-isView", () => ArrayBuffer.isView(...([new Uint8Array(1)] as [any])));
t("url-canParse", () => URL.canParse(...(["/p", "http://h.test"] as [string, string])));
t("uint8-of", () => JSON.stringify(Array.from(Uint8Array.of(...([1, 2] as number[])))));
t("float64-of", () => JSON.stringify(Array.from(Float64Array.of(...([1, 2] as number[])))));
t("int32-from", () => JSON.stringify(Array.from(Int32Array.from(...([[1, 2]] as [number[]])))));
t("array-from-mapped", () => JSON.stringify(Array.from(...([[1, 2], (x: number) => x * 2] as [number[], (x: number) => number]))));
t("array-from-alias-mapped", () => { const af: any = Array.from; return JSON.stringify(af([1, 2], (x: number) => x * 2)); });

// Constructors reached as values.
t("dyn-error-cause", () => { const E = (globalThis as any).Error; return new E("m", { cause: 7 }).cause; });
t("dyn-proxy", () => { const P = (globalThis as any).Proxy; return new P({ a: 1 }, { get: () => 9 }).a; });
t("dyn-aggregate", () => { const A = (globalThis as any).AggregateError; return new A([new Error("a")], "agg").message; });
t("reflect-construct-weakref", () => Reflect.construct(WeakRef, [wr]).deref() === wr);

// Shadowed names construct the binding, not the global.
t("shadow-param", () => ((Map: any, a: any[]) => new Map(...a))(Array, [3]).length);
t("global-bigint-typeof", () => typeof (globalThis as any).BigInt);
t("global-symbol-typeof", () => typeof (globalThis as any).Symbol);
