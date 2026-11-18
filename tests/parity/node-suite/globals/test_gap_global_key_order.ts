// Global object key order as node prints it, for the names perry declares (node-only
// names left out; a name the runtime lacks is skipped, never reordered).
function rel(actual: string[], expected: string[]): boolean {
  const e = new Set(expected), a = new Set(actual);
  const x = actual.filter((k) => e.has(k)), y = expected.filter((k) => a.has(k));
  const ok = x.length === y.length && x.every((k, i) => k === y[i]);
  if (!ok) { const i = x.findIndex((k, j) => k !== y[j]); console.log("first diff", i, x.slice(i, i + 4).join(), "|", y.slice(i, i + 4).join()); }
  return ok;
}
const NAMES = ["Object", "Function", "Array", "Number", "parseFloat", "parseInt", "Infinity", "NaN", "undefined", "Boolean", "String", "Symbol", "Date", "Promise", "RegExp", "Error", "AggregateError", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "globalThis", "JSON", "Math", "Intl", "ArrayBuffer", "Atomics", "Uint8Array", "Int8Array", "Uint16Array", "Int16Array", "Uint32Array", "Int32Array", "BigUint64Array", "BigInt64Array", "Uint8ClampedArray", "Float32Array", "Float64Array", "Float16Array", "DataView", "Map", "BigInt", "Set", "Iterator", "WeakMap", "WeakSet", "Proxy", "Reflect", "FinalizationRegistry", "WeakRef", "decodeURI", "decodeURIComponent", "encodeURI", "encodeURIComponent", "escape", "unescape", "eval", "isFinite", "isNaN", "console", "process", "global", "Buffer", "clearImmediate", "setImmediate", "URL", "URLSearchParams", "DOMException", "AbortController", "AbortSignal", "Event", "EventTarget", "CustomEvent", "TextEncoder", "TextDecoder", "TransformStream", "WritableStream", "ReadableStream", "ReadableStreamDefaultReader", "ReadableStreamBYOBReader", "ReadableStreamBYOBRequest", "ReadableByteStreamController", "ReadableStreamDefaultController", "TextEncoderStream", "TextDecoderStream", "CompressionStream", "DecompressionStream", "clearInterval", "clearTimeout", "setInterval", "setTimeout", "queueMicrotask", "structuredClone", "atob", "btoa", "BroadcastChannel", "MessageChannel", "MessagePort", "Blob", "File", "Performance", "PerformanceEntry", "PerformanceMark", "PerformanceMeasure", "PerformanceObserver", "PerformanceObserverEntryList", "PerformanceResourceTiming", "performance", "URLPattern", "fetch", "FormData", "Headers", "Request", "Response", "MessageEvent", "WebSocket", "crypto", "Crypto", "CryptoKey", "SubtleCrypto", "Temporal", "SuppressedError", "DisposableStack", "AsyncDisposableStack", "SharedArrayBuffer", "WebAssembly", "Navigator", "navigator", "Storage", "localStorage", "sessionStorage"];
const KEYS = ["global", "clearImmediate", "setImmediate", "clearInterval", "clearTimeout", "setInterval", "setTimeout", "queueMicrotask", "structuredClone", "atob", "btoa", "performance", "fetch", "crypto", "navigator", "sessionStorage"];
console.log("names", rel(Object.getOwnPropertyNames(globalThis), NAMES));
console.log("keys", rel(Object.keys(globalThis), KEYS));
const forIn: string[] = [];
for (const k in globalThis) forIn.push(k);
console.log("for-in", rel(forIn, KEYS));
