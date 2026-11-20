import * as fs from "node:fs";
import * as fsp from "node:fs/promises";

const ROOT = "/tmp/perry_node_suite_fs_promises_open_create_errors";
try { await fsp.rm(ROOT, { recursive: true, force: true }); } catch (_e) {}
await fsp.mkdir(ROOT, { recursive: true });
await fsp.mkdir(ROOT + "/dir");

async function expectOpenReject(label: string, target: string, flags: string | number, expectedCode: string) {
  await fsp.open(target, flags).then((fh) => {
    console.log(label + " rejected:", false);
    console.log(label + " fd valid:", fh.fd >= 0);
    try { fh.close(); } catch (_e) {}
  }, (err) => {
    const e = err as any;
    console.log(label + " rejected:", true);
    console.log(label + " error name:", e.name);
    console.log(label + " error code:", e.code);
    console.log(label + " error syscall:", e.syscall);
    console.log(label + " path match:", e.path === target);
    console.log(label + " expected code:", e.code === expectedCode);
  });
}

await expectOpenReject("missing parent write", ROOT + "/missing-parent/file.txt", "w", "ENOENT");
await expectOpenReject(
  "missing parent numeric create",
  ROOT + "/missing-parent/numeric.txt",
  fs.constants.O_CREAT | fs.constants.O_WRONLY | fs.constants.O_TRUNC,
  "ENOENT",
);
await expectOpenReject("directory write", ROOT + "/dir", "w", "EISDIR");
await expectOpenReject("directory readwrite", ROOT + "/dir", "r+", "EISDIR");
