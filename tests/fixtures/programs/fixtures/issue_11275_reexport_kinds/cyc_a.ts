// `export *` cycle: cyc_a <-> cyc_b.
import { obj } from "./origin.ts";
export { obj as cycObj };
export * from "./cyc_b.ts";
