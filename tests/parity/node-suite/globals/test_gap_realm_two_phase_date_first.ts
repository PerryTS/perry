// Two-phase realm: Date is a phase-B family. Its instances are made and
// used by compiled code before the realm is completed; the first read that
// needs %Date.prototype% or the `Date` binding completes it.
class Core { static n = 1; }
const first = [Core];
console.log("core", first[0].n);
const d = new Date(Date.UTC(2020, 0, 2, 3, 4, 5));
const anyD: any = d;
console.log("date direct", d.getTime(), d.toISOString(), Date.now() > 0);
console.log("date dynamic", anyD["getUTCFullYear"](), typeof anyD.toJSON, JSON.stringify({ d }));
console.log("date proto", Object.getPrototypeOf(d) === Date.prototype, Object.prototype.toString.call(d));
console.log("date identity", Date === (globalThis as any).Date, typeof Date.parse, Date.length, Date.name);
console.log("descriptor", JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, "Date"), (k, v) => (typeof v === "function" ? "fn" : v)));
class Stamp extends Date { tag() { return "s" + this.getUTCDate(); } }
console.log("subclass", new Stamp(0).tag(), new Stamp(0) instanceof Date, Object.getPrototypeOf(Stamp) === Date);
console.log("string", String(new Date(0).getTime()), typeof new Date(0).valueOf(), new Date(NaN).getTime());
