// Force realm initialization; console-only startup can dead-strip the installers.
console.log(typeof globalThis.Object, typeof globalThis.Array, typeof globalThis.Map);
