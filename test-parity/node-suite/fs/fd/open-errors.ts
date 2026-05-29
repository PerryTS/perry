import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_open_errors";
const target = ROOT + "/missing-parent/file.txt";

try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}

try {
  fs.openSync(target, "w");
  console.log("sync unexpectedly opened");
} catch (e: any) {
  console.log("sync error:", e instanceof Error, e.code, e.syscall, e.path === target);
}

await new Promise<void>((resolve) => {
  fs.open(target, "w", (err, fd) => {
    console.log(
      "callback error:",
      err instanceof Error,
      err && (err as any).code,
      err && (err as any).syscall,
      err && (err as any).path === target,
      fd === undefined,
    );
    resolve();
  });
});
