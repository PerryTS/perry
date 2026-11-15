import { iso } from "./iso.ts";
import { tbl } from "./work.ts";
gc();
console.log(iso.env.SNAPENV_K ?? "unset", iso.env === process.env, tbl.length, "SNAPENV_K" in iso.env);
