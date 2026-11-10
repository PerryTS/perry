// The ReadableStream global read as a value in a program with no other
// streams use: its declared static `from` is an own, enumerable function,
// exactly as in Node (presence, keys, value and own names agree).
console.log("from" in ReadableStream, Object.keys(ReadableStream), typeof ReadableStream.from, Object.getOwnPropertyNames(ReadableStream).sort().join());
