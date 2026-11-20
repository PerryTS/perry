// Synchronous fs calls that node declares as returning undefined, and
// mkdirSync({ recursive: true }) returning the first directory it created.
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_sync_void";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
const show = (label: string, v: unknown) => console.log(label + ":", typeof v, v);

show("mkdirSync", fs.mkdirSync(ROOT));
show("mkdirSync recursive", fs.mkdirSync(ROOT + "/a/b/c", { recursive: true }));
show("mkdirSync recursive existing", fs.mkdirSync(ROOT + "/a/b", { recursive: true }));
show("mkdirSync recursive one", fs.mkdirSync(ROOT + "/a/b/d", { recursive: true }));
show("writeFileSync", fs.writeFileSync(ROOT + "/f.txt", "hello"));
show("appendFileSync", fs.appendFileSync(ROOT + "/f.txt", "!"));
show("utimesSync", fs.utimesSync(ROOT + "/f.txt", 1000, 2000));
show("copyFileSync", fs.copyFileSync(ROOT + "/f.txt", ROOT + "/g.txt"));
show("renameSync", fs.renameSync(ROOT + "/g.txt", ROOT + "/h.txt"));
show("truncateSync", fs.truncateSync(ROOT + "/h.txt", 2));
show("chmodSync", fs.chmodSync(ROOT + "/h.txt", 0o644));
const fd = fs.openSync(ROOT + "/f.txt", "r+");
show("fsyncSync", fs.fsyncSync(fd));
show("ftruncateSync", fs.ftruncateSync(fd, 3));
show("closeSync", fs.closeSync(fd));
show("unlinkSync", fs.unlinkSync(ROOT + "/h.txt"));
show("rmdirSync", fs.rmdirSync(ROOT + "/a/b/d"));
show("rmSync", fs.rmSync(ROOT, { recursive: true, force: true }));
