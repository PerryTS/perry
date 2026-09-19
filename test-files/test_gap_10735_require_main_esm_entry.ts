// #10735 companion: when the process ENTRY is ESM, no CommonJS module ever
// ran as "main" — a CJS module reached only via `import` from that entry
// must see `require.main === undefined`, not merely "not itself".
import cjsDep from "./gap_10735_require_main_esm_dep.cjs";

console.log("esm entry imported cjs dep:", cjsDep);
