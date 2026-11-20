// Issue #2058: built-in prototype methods read AS PROPERTY VALUES (not called)
// — via the prototype object, or off a primitive receiver — resolved to
// `undefined` (and a primitive receiver SIGSEGV'd). They are real functions in
// Node, so `typeof` must be "function" and the bound value must be callable.

// --- Via the prototype object (Object/Number/String/Array/Function). ---
console.log(typeof Object.prototype.isPrototypeOf);
console.log(typeof Object.prototype.hasOwnProperty);
console.log(typeof Object.prototype.toString);
console.log(typeof Object.prototype.valueOf);
console.log(typeof Object.prototype.propertyIsEnumerable);
console.log(typeof Number.prototype.isPrototypeOf);
console.log(typeof Number.prototype.hasOwnProperty);
console.log(typeof Number.prototype.toFixed);
console.log(typeof String.prototype.isPrototypeOf);
console.log(typeof Array.prototype.hasOwnProperty);

// --- The sibling Function.prototype.{call,apply,bind} gap (the title). ---
console.log(typeof Function.prototype.call);
console.log(typeof Function.prototype.apply);
console.log(typeof Function.prototype.bind);
console.log(typeof Function.prototype.toString);

// --- On a primitive receiver (this previously crashed). ---
var n = 5;
console.log(typeof n.isPrototypeOf, typeof n.hasOwnProperty, typeof n.toString);
console.log(typeof n.valueOf, typeof n.toLocaleString, typeof n.propertyIsEnumerable);

// --- The bound values are actually callable (direct invocation). ---
console.log(n.toString());
console.log(n.valueOf());
console.log(n.isPrototypeOf({}));
