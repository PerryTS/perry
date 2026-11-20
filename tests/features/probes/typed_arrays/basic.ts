const bytes = new Uint8Array([1, 2, 3]);
bytes[1] = 9;

console.log(`typed-array:${bytes.length}:${bytes.join(",")}:${bytes.BYTES_PER_ELEMENT}`);
