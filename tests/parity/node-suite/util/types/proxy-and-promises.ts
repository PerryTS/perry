import { types } from "node:util";

const proxy = new Proxy({}, {});
console.log("proxy:", types.isProxy(proxy));
console.log("promise:", types.isPromise(Promise.resolve(1)));
console.log("map iterator:", types.isMapIterator(new Map().keys()));
console.log("set iterator:", types.isSetIterator(new Set().values()));
console.log("arraybuffer view:", types.isArrayBufferView(new DataView(new ArrayBuffer(1))));
console.log("boxed string:", types.isBoxedPrimitive(new String("x")));
