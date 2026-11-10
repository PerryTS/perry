const O: any = Object;
if (typeof O.hasOwnProperty !== "function") throw new Error("hasOwnProperty");
if (typeof O.isPrototypeOf !== "function") throw new Error("isPrototypeOf");
if (typeof O.propertyIsEnumerable !== "function") throw new Error("propertyIsEnumerable");
const proto = O.getPrototypeOf({ a: 1 });
if (O.hasOwnProperty.call(proto, "constructor") !== true) throw new Error("call(constructor)");
if (O.hasOwnProperty.call(proto, "nope") !== false) throw new Error("call(nope)");
console.log("OK");
