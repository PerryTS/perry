// Issue #3961: `Uint8Array.prototype.buffer` must return the underlying
// ArrayBuffer (not the Uint8Array itself), and `Uint8Array.prototype.slice`
// must return a Uint8Array (not a Node Buffer) whose `.buffer` is likewise a
// real ArrayBuffer.
const arr1 = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9]);

const buffer1 = arr1.buffer;
console.log("direct typeof:", typeof buffer1);
console.log("direct instanceof ArrayBuffer:", buffer1 instanceof ArrayBuffer);
console.log("direct tag:", Object.prototype.toString.call(buffer1));
console.log("direct byteLength:", buffer1.byteLength);

const sliced = arr1.slice();
console.log("slice tag:", Object.prototype.toString.call(sliced));
console.log("slice is Uint8Array:", sliced instanceof Uint8Array);

const buffer2 = sliced.buffer;
console.log("slice.buffer typeof:", typeof buffer2);
console.log("slice.buffer instanceof ArrayBuffer:", buffer2 instanceof ArrayBuffer);
console.log("slice.buffer tag:", Object.prototype.toString.call(buffer2));
console.log("slice.buffer byteLength:", buffer2.byteLength);

// Reading the bytes back out of the returned ArrayBuffer must round-trip.
const reread = new Uint8Array(buffer2);
console.log("reread bytes:", Array.from(reread).join(","));
