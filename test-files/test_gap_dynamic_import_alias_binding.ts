// parity-env: PERRY_GC_SCHEDULE_SEED=9778 PERRY_GC_SCHEDULE_RATE=0.25 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1

async function main(): Promise<void> {
  const ns = await import("./dynamic_import_alias_binding.ts");

  console.log(
    typeof ns.VAR_SET,
    typeof ns.LET_SET,
    typeof ns.CONST_SET,
    typeof ns.directConst,
    typeof ns.checkAlias,
  );
  console.log(
    ns.checkAlias(ns.VAR_SET, "var-before"),
    ns.checkAlias(ns.LET_SET, "let-before"),
    ns.checkAlias(ns.CONST_SET, "const"),
    ns.checkAlias(ns.directConst, "direct"),
  );

  ns.reassign();
  console.log(
    ns.checkAlias(ns.VAR_SET, "var-after"),
    ns.checkAlias(ns.LET_SET, "let-after"),
    ns.checkAlias(ns.VAR_SET, "var-before"),
    ns.checkAlias(ns.LET_SET, "let-before"),
  );
}

main();
