// #11193: `Buffer[Symbol.species]` is Node's internal `FastBuffer`: the
// Uint8Array subclass every Buffer is an instance of. undici 8.9.0 captures it
// at module load (`const FastBuffer = Buffer[Symbol.species]`, in
// lib/dispatcher/client-h1.js) and wraps every llhttp callback argument as
// `new FastBuffer(currentBufferRef.buffer, start, len)`, a Buffer VIEW over
// the parser's memory. Perry answered `undefined`, so the first response
// status line threw "undefined is not a constructor" inside the parser.

const FastBuffer: any = (Buffer as any)[Symbol.species];
console.log("typeof species:", typeof FastBuffer);
console.log("stable identity:", (Buffer as any)[Symbol.species] === FastBuffer);
console.log("not Buffer itself:", FastBuffer !== Buffer);
console.log("name:", FastBuffer.name, "length:", FastBuffer.length);
console.log("shares Buffer.prototype:", FastBuffer.prototype === Buffer.prototype);

const d = Object.getOwnPropertyDescriptor(Buffer, Symbol.species)!;
console.log("own accessor:", typeof d.get, "set:", typeof d.set, "enumerable:", d.enumerable, "configurable:", d.configurable);

// undici's exact use: a view over part of an ArrayBuffer.
const ab = new ArrayBuffer(16);
const bytes = new Uint8Array(ab);
bytes.set([72, 84, 84, 80, 47, 49, 46, 49, 32, 50, 48, 48], 2); // "HTTP/1.1 200"
const status = new FastBuffer(ab, 11, 3);
console.log("view isBuffer:", Buffer.isBuffer(status), "len:", status.length, "text:", status.toString());
console.log("view latin1:", status.toString("latin1"), "byteOffset:", status.byteOffset, "same memory:", status.buffer === ab);
bytes[11] = 52; // "400"
console.log("sees later writes:", status.toString());
status[2] = 49; // "401"
console.log("writes through:", String.fromCharCode(bytes[11], bytes[12], bytes[13]));

// Remaining shapes.
const sized = new FastBuffer(4);
console.log("sized:", Buffer.isBuffer(sized), JSON.stringify([...sized]));
const whole = new FastBuffer(ab);
console.log("whole view len:", whole.length, "same memory:", whole.buffer === ab);
const fromArray = new FastBuffer([1, 2, 3]);
console.log("from array:", Buffer.isBuffer(fromArray), JSON.stringify([...fromArray]));
