// A dynamic function made inside a dependency's init.
const f = new Function("return 1");
export const v = f();
