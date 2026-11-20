import { iso } from "./iso.ts";
export const tbl = Array.from({length: 2000}, (_, i) => i * 3);
export const same = iso.env === iso.env;
