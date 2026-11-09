import { a } from "./m.ts";
// The entry reads argv too: a host value read live, never a STOP.
const mine = process.argv.length;
console.log("K2A_F1", a.length >= 2, mine >= 2);
