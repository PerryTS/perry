// Stats Date fields through every access form a caller uses: statSync, the
// stat/lstat/fstat callbacks, and `fs.promises.*` reached as a member of the
// default `node:fs` import (the form `send` / @astrojs/node use), plus bigint
// Stats. Prints booleans only, so the output does not depend on the clock.
import fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stats_date_fields_access_forms";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });
const file = ROOT + "/file.txt";
fs.writeFileSync(file, "access-forms");

const KEYS = ["atime", "mtime", "ctime", "birthtime"];

function show(label: string, st: any) {
  console.log(`${label} stats object:`, typeof st === "object" && st !== null && typeof st.isFile === "function" && st.isFile());
  console.log(`${label} Date instances:`, KEYS.map((k) => st[k] instanceof Date).join(","));
  console.log(
    `${label} getTime matches *Ms:`,
    KEYS.map((k) => st[k] instanceof Date && st[k].getTime() === Math.round(Number(st[k + "Ms"]))).join(","),
  );
  console.log(`${label} mtime.toUTCString callable:`, typeof st.mtime?.toUTCString === "function");
}

const plain = fs.statSync(file);
const big = fs.statSync(file, { bigint: true });
console.log("constructor names:", plain.constructor.name, big.constructor.name);

// A user class that happens to share the runtime's class name keeps its own
// accessors; the Stats registration must not capture it.
class Stats {
  get mtime() { return "user getter"; }
}
console.log("user Stats class:", new Stats().mtime, new Stats().constructor.name);
console.log("fs Stats unaffected:", plain.mtime instanceof Date);

show("statSync", plain);
show("lstatSync", fs.lstatSync(file));
const fd = fs.openSync(file, "r");
show("fstatSync", fs.fstatSync(fd));
show("statSync bigint", big);

fs.stat(file, (err, st) => {
  console.log("stat callback err:", err === null);
  show("stat callback", st);
  fs.lstat(file, (lerr, lst) => {
    console.log("lstat callback err:", lerr === null);
    show("lstat callback", lst);
    fs.fstat(fd, (ferr, fst) => {
      console.log("fstat callback err:", ferr === null);
      show("fstat callback", fst);
      fs.closeSync(fd);
      void promises();
    });
  });
});

async function promises() {
  show("fs.promises.stat", await fs.promises.stat(file));
  show("fs.promises.lstat", await fs.promises.lstat(file));
  show("fs.promises.stat bigint", await fs.promises.stat(file, { bigint: true }));
  fs.rmSync(ROOT, { recursive: true, force: true });
}
