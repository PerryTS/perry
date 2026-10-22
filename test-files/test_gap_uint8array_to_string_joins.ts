// A plain Uint8Array stringifies through %TypedArray%.prototype.toString
// (the element join). Only Buffer overrides toString with the UTF-8 decode.
const u = new Uint8Array([1, 2, 3, 4, 5]);
console.log(String(u));
console.log(String(u.slice(1, 3)));
console.log(String(u.subarray(2)));
console.log(`${u.slice(0, 2)}`);
console.log("" + u.slice(3));
console.log(u.slice(1).toString());
console.log(u.toString("hex" as any));
console.log(String(new Uint8Array(0)) === "");
console.log([u.slice(0, 1), u.slice(4)].join("|"));
console.log(String(new Uint8Array([72, 105])));
console.log(String(new Uint8Array(new ArrayBuffer(3))));
console.log(String(Uint8Array.from([250, 251])));
console.log(String(new Int16Array([-1, 300]).slice(0)));
// Buffer keeps its own toString.
const b = Buffer.from([72, 105]);
console.log(String(b), b.toString("hex"), `${b.subarray(1)}`);
console.log(String(Buffer.from("ok").slice(0, 1)));
