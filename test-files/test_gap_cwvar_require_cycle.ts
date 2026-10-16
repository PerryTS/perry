// Closed World: a CommonJS require cycle re-enters a module before its
// module-level var is assigned; readers that escaped before it see undefined.
import a from "./cwvar/cycle_a.cjs";

console.log("early(closure)", a.early);
console.log("early(named)", a.earlyNamed);
console.log("late", a.late, a.read(), a.readNamed());
