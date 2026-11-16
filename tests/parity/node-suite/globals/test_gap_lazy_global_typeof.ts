// typeof and plain value reads of global functions and namespaces.
console.log(typeof setTimeout, typeof clearTimeout, typeof setInterval, typeof clearInterval);
console.log(typeof setImmediate, typeof clearImmediate, typeof queueMicrotask, typeof structuredClone);
console.log(typeof fetch, typeof atob, typeof btoa, typeof eval, typeof WebAssembly);
console.log(typeof parseInt, typeof parseFloat, typeof isNaN, typeof isFinite);
console.log(typeof encodeURI, typeof decodeURI, typeof encodeURIComponent, typeof decodeURIComponent);
console.log(typeof escape, typeof unescape);
const p = parseInt;
const enc = encodeURIComponent;
console.log("values", p("42px"), enc("x y"), p === Number.parseInt, parseFloat === Number.parseFloat);
console.log("names", setTimeout.name, structuredClone.length, isNaN.name, WebAssembly.validate.name);
