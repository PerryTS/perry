// A dynamic function made inside a dependency's init (source text only known at run time), kept in an exported binding.
export const f = new Function("return " + (process.argv.length > 0 ? 1 : 2));
export const v = f();
