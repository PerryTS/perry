// A program with a dynamic `import()` must not get `globalThis.AsyncLocalStorage`:
// node has no such global (only Next.js assigns one). The entry seeds it for
// Next.js server modules alone; a module reached only through `import()` used
// to put the program on that path, so this printed "function".

const m = await import("./dynamic_import_als_seed_target.ts");
console.log(typeof (globalThis as any).AsyncLocalStorage, m.x);
console.log("AsyncLocalStorage" in globalThis);
