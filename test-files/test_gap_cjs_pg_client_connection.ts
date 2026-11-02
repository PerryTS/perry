// pg's CommonJS Client/Connection shape: a constructor function that stores
// the classes on its instance (`this.Client = clientConstructor`), a lazy
// `native` getter replaced by a value on first read, a Client that deletes
// its own bookkeeping by a computed key in an error handler, and a
// Connection that is an EventEmitter subclass. Whole-program facts must keep
// every one of these live and exact.
import * as pg from "./cjs2_shapes/index.cjs";

const mod: any = (pg as any).default ?? pg;
console.log(typeof mod.Client, typeof mod.Connection, mod.defaults.port);
const c = new mod.Client({ user: "u" });
let errors = 0;
c.on("error", (e: Error) => { errors++; console.log("error event:", e.message); });
c.on("end", () => console.log("end event"));
c.connect();
c.connection.sendError("relation does not exist");
console.log(JSON.stringify(c.pending), errors);
console.log(c.log.join(","));
c.end();
console.log(c.connection._ending, c instanceof mod.Client);
console.log(mod.types.getTypeParser(23)("7"));
console.log(mod.native.loaded, mod.native.loaded);
const con = new mod.Connection();
con.on("end", () => console.log("bare connection end"));
con.end();
