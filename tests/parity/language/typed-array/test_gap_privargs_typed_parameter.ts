// Admission belongs to the receiver, even after unrelated view creation.
function byte(a: Uint8Array, i: number) { return a[i]; }
function word(a: Uint32Array, i: number) { return a[i]; }
function bufferByte(a: Buffer, i: number) { return a[i]; }
const bytes = new Uint8Array([3, 7, 11]);
const words = new Uint32Array([13, 17, 19]);
const buffer = Buffer.from([23, 29, 31]);
console.log('before', byte(bytes, 1), word(words, 1), bufferByte(buffer, 1));
const backing = new ArrayBuffer(16);
const aliasBytes = new Uint8Array(backing);
const aliasWords = new Uint32Array(backing);
aliasWords[0] = 257;
console.log('aliases', byte(aliasBytes, 0), byte(aliasBytes, 1), word(aliasWords, 0));
aliasBytes[0] = 5;
console.log('updated', word(aliasWords, 0));
console.log('after', byte(bytes, 1), word(words, 1), bufferByte(buffer, 1));
for (const index of [-1, 3, 0.5, NaN, Infinity]) {
    console.log('bounds', byte(bytes, index), word(words, index), bufferByte(buffer, index));
}
// Type annotations do not authorize an unchecked cell load.
console.log('ordinary', byte([37] as any, 0), word({ 0: 41 } as any, 0));
