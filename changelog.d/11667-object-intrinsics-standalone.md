Made the first use of a class cheaper: `Object` and `Object.prototype` are
built on their own (about 1M instructions) instead of by building the whole
global object (about 50M instructions, several hundred builtins); the global
object adopts the same two objects when it is built.
