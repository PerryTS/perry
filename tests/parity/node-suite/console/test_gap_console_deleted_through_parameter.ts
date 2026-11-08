// `delete console.log` through a parameter removes the member: the direct call throws.
function del(o: any, k: string): boolean { return delete o[k]; }
const deleted = del(console, process.argv.length > 99 ? "x" : "log");
const c: any = console;
process.stdout.write("deleted " + deleted + " in " + ("log" in c) + " own " + Object.prototype.hasOwnProperty.call(c, "log") + " keys " + Object.keys(c).includes("log") + " typeof " + typeof c.log + "\n");
try { console.log("hello"); process.stdout.write("no throw\n"); } catch (e) { process.stdout.write("threw " + (e as Error).constructor.name + "\n"); }
console.error("error still works");
c.log = (s: string) => { process.stdout.write("restored " + s + "\n"); };
console.log("hello");
