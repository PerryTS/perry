import { Utf8Stream } from "node:fs";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_utf8stream_validation";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

function report(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ":", "ok");
  } catch (e: any) {
    console.log(label + ":", e.name, e.code, e instanceof TypeError, e instanceof RangeError);
  }
}

const p = ROOT + "/validation.txt";
const url = new URL("file://" + p);

report("null options", () => new Utf8Stream(null as any));
report("missing fd dest", () => new Utf8Stream({} as any));
report("buffer dest", () => new Utf8Stream({ dest: Buffer.from(p), sync: true } as any));
report("url dest", () => new Utf8Stream({ dest: url, sync: true } as any));
report("bad content mode", () => new Utf8Stream({ dest: p, contentMode: "latin1", sync: true } as any));
report("bad bool", () => new Utf8Stream({ dest: p, sync: 1 } as any));
report("bad uint", () => new Utf8Stream({ dest: p, minLength: -1, sync: true } as any));
report("bad min max", () => new Utf8Stream({ dest: p, minLength: 4, maxWrite: 4, sync: true } as any));
report("bad retry", () => new Utf8Stream({ dest: p, retryEAGAIN: 1, sync: true } as any));
report("bad custom fs", () => new Utf8Stream({ dest: p, fs: { writeSync: 1 }, sync: true } as any));

const utf8Type = new Utf8Stream({ dest: ROOT + "/utf8-type.txt", sync: true });
report("utf8 write buffer", () => utf8Type.write(Buffer.from("x") as any));
utf8Type.destroy();

const bufferType = new Utf8Stream({ dest: ROOT + "/buffer-type.txt", contentMode: "buffer", sync: true });
report("buffer write string", () => bufferType.write("x" as any));
bufferType.destroy();

const fd = fs.openSync(ROOT + "/fd-only.txt", "w");
const fdStream = new Utf8Stream({ fd, sync: true });
report("fd only reopen", () => fdStream.reopen(ROOT + "/fd-reopen.txt"));
fdStream.destroy();
