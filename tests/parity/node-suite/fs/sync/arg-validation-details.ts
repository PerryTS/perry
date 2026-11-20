import * as fs from "node:fs";

function probe(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, "name", err.name);
    console.log(label, "code", err.code);
    console.log(label, "syscall", String(err.syscall));
    console.log(label, "path", String(err.path));
    console.log(label, "message", err.message);
  }
}

probe("sync path bool", () => fs.truncateSync(true as any, 0));
probe("sync options number", () => fs.readFileSync("/tmp/perry_sync_arg_validation_missing", 5 as any));
probe("sync options array", () => fs.rmSync("/tmp/perry_sync_arg_validation_missing", [] as any));
probe("sync mode string", () => fs.accessSync("/tmp", "x" as any));
probe("sync mode range", () => fs.accessSync("/tmp", 8));
probe("sync copy mode string", () =>
  fs.copyFileSync("/tmp/perry_sync_arg_validation_missing_a", "/tmp/perry_sync_arg_validation_missing_b", "x" as any),
);
probe("sync fd type", () => fs.ftruncateSync("" as any, 0));
probe("sync fd ebadf", () => fs.ftruncateSync(987654321, 0));
probe("sync fd futime ebadf", () => fs.futimesSync(987654321, 1, 1));
