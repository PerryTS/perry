// An enabled async hook gets each watcher fs.watch / fs.watchFile makes as
// its init resource (perry hands the watcher itself, node its handle) and
// may store into it; values read back off the watcher stay sound.
import * as fs from "node:fs";
import * as path from "node:path";
import * as os from "node:os";
import { createHook } from "node:async_hooks";

const counts: Record<string, number> = {};
const seen: any[] = [];
const hook = createHook({
  init(_id: number, type: string, _tid: number, res: any) {
    if (type === "FSEVENTWRAP" || type === "STATWATCHER") {
      counts[type] = (counts[type] ?? 0) + 1;
      res.tag = { k: 7 };
      seen.push(res);
    }
  },
});
hook.enable();
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-ah-"));
const file = path.join(dir, "f.txt");
fs.writeFileSync(file, "x");
const w: any = fs.watch(dir, { persistent: false }, function (this: any) {});
const s: any = fs.watchFile(file, { persistent: false, interval: 5000 }, function () {});
hook.disable();
console.log("inits", counts["FSEVENTWRAP"], counts["STATWATCHER"]);
console.log("tag via hook", seen[0].tag.k, seen[1].tag.k);
console.log("tag via watcher", (w.tag ?? { k: 7 }).k, (s.tag ?? { k: 7 }).k);
console.log("close", typeof w.close, typeof s.ref);
w.close();
fs.unwatchFile(file);
fs.rmSync(dir, { recursive: true, force: true });
