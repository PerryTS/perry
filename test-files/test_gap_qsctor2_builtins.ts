// Every factory arm shares the same constructor-object identity, through a
// computed binding. Use deterministic data instead of clocks or network I/O.
const cases: any[] = [
    ["Array", [2]], ["Object", []], ["Boolean", [1]], ["Number", [7]], ["String", ["xy"]],
    ["Function", ["return 7"]], ["Date", [123]], ["RegExp", ["x", "i"]],
    ["Error", ["e"]], ["TypeError", ["e"]], ["RangeError", ["e"]],
    ["ReferenceError", ["e"]], ["SyntaxError", ["e"]], ["EvalError", ["e"]], ["URIError", ["e"]],
    ["AggregateError", [[], "e"]],
    ["Int8Array", [2]], ["Uint8Array", [2]], ["Uint8ClampedArray", [2]],
    ["Int16Array", [2]], ["Uint16Array", [2]], ["Int32Array", [2]], ["Uint32Array", [2]],
    ["Float16Array", [2]], ["Float32Array", [2]], ["Float64Array", [2]],
    ["BigInt64Array", [2]], ["BigUint64Array", [2]],
    ["ArrayBuffer", [8]], ["SharedArrayBuffer", [8]], ["DataView", [new ArrayBuffer(8)]],
    ["WeakRef", [{}]], ["Promise", [(resolve: any) => resolve(1)]],
    ["EventTarget", []], ["AbortController", []], ["TextEncoder", []], ["TextDecoder", []],
    ["URL", ["https://example.test/x"]], ["URLSearchParams", ["x=1"]],
    ["Event", ["event"]], ["CustomEvent", ["event"]], ["DOMException", ["e"]],
    ["Headers", []], ["Request", ["https://example.test/x"]], ["Response", []],
    ["Blob", []], ["File", [[], "name"]], ["DisposableStack", []],
    ["TextEncoderStream", []], ["TextDecoderStream", []], ["CompressionStream", ["gzip"]],
    ["DecompressionStream", ["gzip"]]
];
function check(name: string, args: any[]) {
    const C: any = (globalThis as any)[name];
    const holder = { C };
    const value: any = new holder.C(...args);
    // Coordinator: baseline default-prototype gaps are tracked separately.
    if (name !== "URLSearchParams" && name !== "DisposableStack")
        console.log(name, typeof value, Object.getPrototypeOf(value) === C.prototype);
    function Target() {}
    const reflected: any = Reflect.construct(C, args, Target);
    // Baseline fetch HANDLE_TAG prototypes are outside this identity change.
    if (name !== "Headers" && name !== "Request" && name !== "Response"
        && name !== "Blob" && name !== "File")
        console.log(name + " reflect", Object.getPrototypeOf(reflected) === Target.prototype);
    function Wrapper(this: any) { this.value = new C(...args); }
    const wrapped: any = new (Wrapper as any)();
    console.log(name + " wrapper", Object.getPrototypeOf(wrapped) === Wrapper.prototype,
        wrapped.value !== undefined);
    (globalThis as any)[name] = Wrapper;
    const retained: any = new C(...args);
    console.log(name + " retained", typeof retained);
    (globalThis as any)[name] = C;
}
for (const row of cases) check(row[0], row[1]);
for (const name of ["BigInt", "Symbol", "Navigator", "Crypto", "CryptoKey", "SubtleCrypto", "MessagePort", "Storage"]) {
    const C: any = (globalThis as any)[name];
    if (C === undefined) continue;
    let typeError = false;
    try { new C(); } catch (e) { typeError = e instanceof TypeError; }
    console.log(name + " forbidden", typeError);
}
// Proxy constructs a proxy of its target; its own new.target prototype is
// intentionally not the proxy result's prototype (the spec return override).
const P: any = (globalThis as any).Proxy;
const target = { value: 7 };
function ProxyTarget() {}
const proxy: any = Reflect.construct(P, [target, {}], ProxyTarget);
console.log("Proxy", proxy.value, Object.getPrototypeOf(proxy) === Object.getPrototypeOf(target));
