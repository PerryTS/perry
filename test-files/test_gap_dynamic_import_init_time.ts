const m = await import("./dynamic_import_init_cycle_target.ts");
console.log(m.payload);
