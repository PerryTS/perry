// Issue follow-up to #978: expose Object.prototype methods on Buffer instances.
// safer-buffer (used by express) does `if (buffer.hasOwnProperty(...))` to
// probe Buffer instances. Pre-fix this crashed with
// "buffer.hasOwnProperty is not a function".
const b = Buffer.from('hello', 'utf8');
console.log(b.hasOwnProperty('length'));       // false (length is on prototype)
console.log(b.hasOwnProperty('nonexistent'));  // false
console.log(typeof b.toString);                // 'function'
console.log(b.toString('utf8'));               // 'hello'
console.log(typeof b.valueOf);                 // 'function'
console.log(b.propertyIsEnumerable('length')); // false
