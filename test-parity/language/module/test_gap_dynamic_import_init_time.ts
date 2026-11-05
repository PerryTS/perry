const m = await import("../../../test-files/dynamic_import_init_cycle_target.ts");
console.log(m.payload);
