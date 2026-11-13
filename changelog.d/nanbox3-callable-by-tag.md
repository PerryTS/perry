`new Promise(executor)` and `JSON.parse(text, reviver)` decode the callable by
its tag alone. Codegen masked the argument's tag off unconditionally, so a
Number kept its low 48 bits as an "address" (an address-shaped denormal, its
negative, also `1.1`) and the runtime called it: SIGSEGV. A Number executor now
reaches the runtime as null and throws the TypeError; a reviver that is not an
allocator-owned closure (a Number, an object, an array) is ignored, as in Node.
