import * as realm_native_0 from "node:assert";
import * as realm_native_1 from "node:assert/strict";
import * as realm_native_2 from "node:async_hooks";
import * as realm_native_3 from "node:buffer";
import * as realm_native_4 from "node:child_process";
import * as realm_native_5 from "node:cluster";
import * as realm_native_6 from "node:console";
import * as realm_native_7 from "node:constants";
import * as realm_native_8 from "node:crypto";
import * as realm_native_9 from "node:dgram";
import * as realm_native_10 from "node:dns";
import * as realm_native_11 from "node:dns/promises";
import * as realm_native_12 from "node:domain";
import * as realm_native_13 from "node:events";
import * as realm_native_14 from "node:fs";
import * as realm_native_15 from "node:http";
import * as realm_native_16 from "node:http2";
import * as realm_native_17 from "node:https";
import * as realm_native_18 from "node:inspector";
import * as realm_native_19 from "node:inspector/promises";
import * as realm_native_20 from "node:module";
import * as realm_native_21 from "node:net";
import * as realm_native_22 from "node:os";
import * as realm_native_23 from "node:path";
import * as realm_native_24 from "node:path/posix";
import * as realm_native_25 from "node:path/win32";
import * as realm_native_26 from "node:perf_hooks";
import * as realm_native_27 from "node:process";
import * as realm_native_28 from "node:querystring";
import * as realm_native_29 from "node:readline";
import * as realm_native_30 from "node:repl";
import * as realm_native_31 from "node:sea";
import * as realm_native_32 from "node:stream";
import * as realm_native_33 from "node:streams";
import * as realm_native_34 from "node:string_decoder";
import * as realm_native_35 from "node:test";
import * as realm_native_36 from "node:timers";
import * as realm_native_37 from "node:timers/promises";
import * as realm_native_38 from "node:tls";
import * as realm_native_39 from "node:tty";
import * as realm_native_40 from "node:url";
import * as realm_native_41 from "node:util";
import * as realm_native_42 from "node:util/types";
import * as realm_native_43 from "node:v8";
import * as realm_native_44 from "node:vm";
import * as realm_native_45 from "node:wasi";
import * as realm_native_46 from "node:worker_threads";
import * as realm_native_47 from "node:zlib";
// Reflection oracle for the compiled realm. The descriptor walk never invokes
// accessors; the explicit pristine intrinsic seeds below use known builtins.
// An ES module keeps the dumper's own declarations off the global object.
export {};

const symbolValues: any[] = [];
// Use static accesses: a compiler may expose a well-known symbol intrinsically
// even when the legacy Symbol constructor omits its reflective own property.
const wellKnownSymbols: any[] = [
    ["iterator", Symbol.iterator], ["asyncIterator", Symbol.asyncIterator],
    ["toStringTag", Symbol.toStringTag], ["species", Symbol.species],
    ["hasInstance", Symbol.hasInstance], ["toPrimitive", Symbol.toPrimitive],
    ["match", Symbol.match], ["matchAll", Symbol.matchAll], ["replace", Symbol.replace],
    ["search", Symbol.search], ["split", Symbol.split], ["isConcatSpreadable", Symbol.isConcatSpreadable],
    ["unscopables", Symbol.unscopables], ["dispose", Symbol.dispose], ["asyncDispose", Symbol.asyncDispose],
];
function symbolIdentity(symbol: any): string {
    for (let i = 0; i < wellKnownSymbols.length; i++) {
        if (wellKnownSymbols[i][1] === symbol) return "well-known:" + wellKnownSymbols[i][0];
    }
    const registered = Symbol.keyFor(symbol);
    if (registered !== undefined) return "for:" + registered;
    for (let i = 0; i < symbolValues.length; i++) if (symbolValues[i] === symbol) return "unique:" + i;
    const identity = "unique:" + symbolValues.length;
    symbolValues.push(symbol);
    return identity;
}
const seen: any[] = [];
const paths: string[] = [];
function primitive(v: any): any {
    if (typeof v === "number") {
        if (Number.isNaN(v)) return { number: "NaN" };
        if (v === Infinity) return { number: "+Infinity" };
        if (v === -Infinity) return { number: "-Infinity" };
        if (Object.is(v, -0)) return { number: "-0" };
    }
    if (typeof v === "undefined") return { type: "undefined" };
    if (typeof v === "symbol") return { type: "symbol", value: String(v), identity: symbolIdentity(v) };
    if (typeof v === "bigint") return { type: typeof v, value: String(v) };
    return v;
}
function ref(v: any, path: string): any {
    if (v === null || (typeof v !== "object" && typeof v !== "function")) return primitive(v);
    for (let i = 0; i < seen.length; i++) {
        if (seen[i] === v) return { ref: paths[i] };
    }
    seen.push(v);
    paths.push(path);
    return { ref: path };
}
function descriptor(d: any, path: string): any {
    const out: any = { configurable: d.configurable, enumerable: d.enumerable };
    if (Object.prototype.hasOwnProperty.call(d, "value")) {
        out.writable = d.writable;
        out.value = ref(d.value, path);
    } else {
        out.get = ref(d.get, path + ".<get>");
        out.set = ref(d.set, path + ".<set>");
    }
    return out;
}
// Root keys are sorted before traversal, so aliases/cycles have canonical paths.
// The runner restricts the *result* to R; every arm executes this same program.
const root: any = globalThis;
ref(root, "$global");
// These realm objects are reached through syntax/operations, rather than an
// own property of globalThis. The instances used to find them are not dumped.
const intrinsicRoots: any[] = [];
function intrinsic(name: string, value: any): void {
    intrinsicRoots.push({ name: name, value: ref(value, "$intrinsic[" + JSON.stringify(name) + "]") });
}
const arrayIteratorPrototype = Object.getPrototypeOf([][Symbol.iterator]());
intrinsic("%ArrayIteratorPrototype%", arrayIteratorPrototype);
intrinsic("%IteratorPrototype%", Object.getPrototypeOf(arrayIteratorPrototype));
intrinsic("%StringIteratorPrototype%", Object.getPrototypeOf("x"[Symbol.iterator]()));
intrinsic("%MapIteratorPrototype%", Object.getPrototypeOf(new Map()[Symbol.iterator]()));
intrinsic("%SetIteratorPrototype%", Object.getPrototypeOf(new Set()[Symbol.iterator]()));
intrinsic("%TypedArray%", Object.getPrototypeOf(Uint8Array));
intrinsic("%TypedArray%.prototype", Object.getPrototypeOf(Uint8Array.prototype));
const generator = function* () { yield 1; };
intrinsic("%GeneratorFunctionPrototype%", Object.getPrototypeOf(generator));
intrinsic("%GeneratorPrototype%", Object.getPrototypeOf(generator.prototype));
const asyncGenerator = async function* () { yield 1; };
intrinsic("%AsyncGeneratorFunctionPrototype%", Object.getPrototypeOf(asyncGenerator));
const asyncGeneratorPrototype = Object.getPrototypeOf(asyncGenerator.prototype);
intrinsic("%AsyncGeneratorPrototype%", asyncGeneratorPrototype);
intrinsic("%AsyncIteratorPrototype%", Object.getPrototypeOf(asyncGeneratorPrototype));
intrinsic("%AsyncFunctionPrototype%", Object.getPrototypeOf(async function () { return 1; }));
// The runtime's Buffer species getter captures an intrinsic constructor which
// need not be the later public native-callable Buffer export. Include that
// identity rather than silently omitting an image-to-image capture edge.
const bufferSpecies = Object.getOwnPropertyDescriptor(Buffer, Symbol.species);
if (bufferSpecies && bufferSpecies.get) intrinsic("%BufferSpeciesConstructor%", bufferSpecies.get.call(Buffer));
// Static native imports are realm roots too. Keep literal specifiers so each
// module's existing installer is linked by the qualified reference compiler.
const nativeRoots: any[] = [
    ["node:assert", realm_native_0],
    ["node:assert/strict", realm_native_1],
    ["node:async_hooks", realm_native_2],
    ["node:buffer", realm_native_3],
    ["node:child_process", realm_native_4],
    ["node:cluster", realm_native_5],
    ["node:console", realm_native_6],
    ["node:constants", realm_native_7],
    ["node:crypto", realm_native_8],
    ["node:dgram", realm_native_9],
    ["node:dns", realm_native_10],
    ["node:dns/promises", realm_native_11],
    ["node:domain", realm_native_12],
    ["node:events", realm_native_13],
    ["node:fs", realm_native_14],
    ["node:http", realm_native_15],
    ["node:http2", realm_native_16],
    ["node:https", realm_native_17],
    ["node:inspector", realm_native_18],
    ["node:inspector/promises", realm_native_19],
    ["node:module", realm_native_20],
    ["node:net", realm_native_21],
    ["node:os", realm_native_22],
    ["node:path", realm_native_23],
    ["node:path/posix", realm_native_24],
    ["node:path/win32", realm_native_25],
    ["node:perf_hooks", realm_native_26],
    ["node:process", realm_native_27],
    ["node:querystring", realm_native_28],
    ["node:readline", realm_native_29],
    ["node:repl", realm_native_30],
    ["node:sea", realm_native_31],
    ["node:stream", realm_native_32],
    ["node:streams", realm_native_33],
    ["node:string_decoder", realm_native_34],
    ["node:test", realm_native_35],
    ["node:timers", realm_native_36],
    ["node:timers/promises", realm_native_37],
    ["node:tls", realm_native_38],
    ["node:tty", realm_native_39],
    ["node:url", realm_native_40],
    ["node:util", realm_native_41],
    ["node:util/types", realm_native_42],
    ["node:v8", realm_native_43],
    ["node:vm", realm_native_44],
    ["node:wasi", realm_native_45],
    ["node:worker_threads", realm_native_46],
    ["node:zlib", realm_native_47],
];
for (let i = 0; i < nativeRoots.length; i++) intrinsic(nativeRoots[i][0], nativeRoots[i][1]);
for (let cursor = 0; cursor < seen.length; cursor++) {
    const value = seen[cursor];
    const path = paths[cursor];
    const record: any = { path: path, type: typeof value };
    if (cursor === 0) record.intrinsics = intrinsicRoots;
    const names = Object.getOwnPropertyNames(value);
    record.ownKeyOrder = names.slice();
    names.sort();
    const properties: any[] = [];
    for (let i = 0; i < names.length; i++) {
        const name = names[i];
        const d = Object.getOwnPropertyDescriptor(value, name);
        if (d) {
            const result = descriptor(d, path + "[" + JSON.stringify(name) + "]");
            // These are per-process state, never initial realm constants. Keep
            // their descriptor and type in the oracle, but not a particular PID.
            if ((value === root.process || value === realm_native_27) && (name === "pid" || name === "ppid")) {
                result.value = { dynamic: "process." + name, type: typeof d.value };
            }
            properties.push({ key: name, descriptor: result });
        }
    }
    const symbols = Object.getOwnPropertySymbols(value);
    const symbolOrder: string[] = [];
    for (let i = 0; i < symbols.length; i++) symbolOrder.push(String(symbols[i]));
    record.ownSymbolOrder = symbolOrder;
    record.ownSymbolIdentityOrder = symbols.map((s: any) => symbolIdentity(s));
    const sortedSymbols: any[] = [];
    for (let i = 0; i < symbols.length; i++) sortedSymbols.push(symbols[i]);
    sortedSymbols.sort((a: any, b: any) => String(a) < String(b) ? -1 : String(a) > String(b) ? 1 : 0);
    for (let i = 0; i < sortedSymbols.length; i++) {
        const symbol = sortedSymbols[i];
        const key = String(symbol);
        const d = Object.getOwnPropertyDescriptor(value, symbol);
        if (d) properties.push({ symbol: key, symbolIdentity: symbolIdentity(symbol), descriptor: descriptor(d, path + "[" + key + "]") });
    }
    record.properties = properties;
    record.prototype = ref(Object.getPrototypeOf(value), path + ".<prototype>");
    if (typeof value === "function") {
        record.name = primitive(value.name);
        record.length = primitive(value.length);
    }
    // toStringTag is covered as an own symbol descriptor, including its getter.
    console.log("@realm " + JSON.stringify(record));
}
