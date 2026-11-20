// Indirect eval deletes console.log: the direct call throws.
const src = "delete console.log";
(0, eval)(src);
try { console.log("hello"); process.stdout.write("no throw\n"); } catch (e) { process.stdout.write("threw " + (e as Error).constructor.name + "\n"); }
process.stdout.write("typeof " + typeof (console as any).log + " in " + ("log" in console) + "\n");
