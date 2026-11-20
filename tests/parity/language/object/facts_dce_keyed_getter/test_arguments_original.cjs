// A18: generic readers of a moved (keyed) mapped index, and keys past the inline count.
function p1(a, b) { Object.defineProperty(arguments, 0, { configurable: false }); a = 7; const o = {}; for (const k in arguments) o[k] = arguments[k];
  return [Object.values(arguments).join(","), JSON.stringify(Object.entries(arguments)), JSON.stringify({ ...arguments }), JSON.stringify(o), Array.from(arguments).join(","), arguments[0]].join("|"); }
console.log("p1", p1(1, 2));
function p2(a) { arguments[3] = 9; return [JSON.stringify(Object.getOwnPropertyDescriptor(arguments, 3)), Object.prototype.hasOwnProperty.call(arguments, 3), delete arguments[3], 3 in arguments, arguments.length].join("|"); }
console.log("p2", p2(1));
function p3(a, b) { Object.defineProperty(arguments, 1, { value: 4 }); const r = b; Object.defineProperty(arguments, 1, { configurable: false, value: 6 }); const s = b; b = 8;
  return [r, s, arguments[1], JSON.stringify(Object.getOwnPropertyDescriptor(arguments, 1))].join("|"); }
console.log("p3", p3(1, 2));
function p4(a, b) { Object.defineProperty(arguments, 0, { enumerable: false }); Object.defineProperty(arguments, 0, { get() { return 42; } }); a = 3; return [arguments[0], a, Object.keys(arguments).join(",")].join("|"); }
console.log("p4", p4(1, 2));
function p5(a, b) { Object.defineProperty(arguments, 1, { enumerable: false }); delete arguments[1]; b = 5; return [arguments[1], 1 in arguments, Object.getOwnPropertyNames(arguments).join(",")].join("|"); }
console.log("p5", p5(1, 2));
