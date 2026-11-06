// URI abstract ToString rejects a Symbol, including one returned by a hook.
function completion(run: () => unknown): string {
    try { run(); return "returned"; }
    catch (e) { return e instanceof TypeError ? "TypeError" : "other"; }
}
console.log("encodeURI-direct", completion(() => encodeURI(Symbol("x"))));
console.log("encodeURI-toString", completion(() => encodeURI(({ toString() { return Symbol("x"); } }))));
console.log("encodeURI-toPrimitive", completion(() => encodeURI(({ [Symbol.toPrimitive]() { return Symbol("x"); } }))));
console.log("decodeURI-direct", completion(() => decodeURI(Symbol("x"))));
console.log("decodeURI-toString", completion(() => decodeURI(({ toString() { return Symbol("x"); } }))));
console.log("decodeURI-toPrimitive", completion(() => decodeURI(({ [Symbol.toPrimitive]() { return Symbol("x"); } }))));
console.log("encodeURIComponent-direct", completion(() => encodeURIComponent(Symbol("x"))));
console.log("encodeURIComponent-toString", completion(() => encodeURIComponent(({ toString() { return Symbol("x"); } }))));
console.log("encodeURIComponent-toPrimitive", completion(() => encodeURIComponent(({ [Symbol.toPrimitive]() { return Symbol("x"); } }))));
console.log("decodeURIComponent-direct", completion(() => decodeURIComponent(Symbol("x"))));
console.log("decodeURIComponent-toString", completion(() => decodeURIComponent(({ toString() { return Symbol("x"); } }))));
console.log("decodeURIComponent-toPrimitive", completion(() => decodeURIComponent(({ [Symbol.toPrimitive]() { return Symbol("x"); } }))));
console.log("explicit-string", String(Symbol("x")));
