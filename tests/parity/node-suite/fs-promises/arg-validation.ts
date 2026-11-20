import * as fsp from "node:fs/promises";
import * as fs from "node:fs";

async function probe(label: string, fn: () => Promise<unknown>) {
  let promise: Promise<unknown>;
  try {
    promise = fn();
    console.log(label, "sync", "returned");
  } catch (err: any) {
    console.log(label, "sync", "threw");
    console.log(label, "name", err.name);
    console.log(label, "code", err.code);
    console.log(label, "syscall", String(err.syscall));
    console.log(label, "path", String(err.path));
    console.log(label, "message", err.message);
    return;
  }

  try {
    await promise;
    console.log(label, "resolved");
  } catch (err: any) {
    console.log(label, "name", err.name);
    console.log(label, "code", err.code);
    console.log(label, "syscall", String(err.syscall));
    console.log(label, "path", String(err.path));
    console.log(label, "message", err.message);
  }
}

await probe("promises readFile options number", () =>
  fsp.readFile("/tmp/perry_promises_arg_validation_missing", 5 as any),
);
await probe("fs.promises readFile options number", () =>
  fs.promises.readFile("/tmp/perry_promises_arg_validation_missing", 5 as any),
);
await probe("promises writeFile options number", () =>
  fsp.writeFile("/tmp/perry_promises_arg_validation_write", "x", 5 as any),
);
await probe("fs.promises writeFile options number", () =>
  fs.promises.writeFile("/tmp/perry_promises_arg_validation_write", "x", 5 as any),
);
await probe("promises appendFile options number", () =>
  fsp.appendFile("/tmp/perry_promises_arg_validation_write", "x", 5 as any),
);
await probe("fs.promises appendFile options number", () =>
  fs.promises.appendFile("/tmp/perry_promises_arg_validation_write", "x", 5 as any),
);
await probe("promises access mode string", () => fsp.access("/tmp", "x" as any));
await probe("promises access mode range", () => fsp.access("/tmp", 8));
await probe("promises copyFile mode string", () =>
  fsp.copyFile("/tmp/perry_promises_arg_validation_missing_a", "/tmp/perry_promises_arg_validation_missing_b", "x" as any),
);
await probe("promises chmod path bool", () => fsp.chmod(true as any, 0o600));
await probe("promises chown uid string", () => fsp.chown("/tmp", "x" as any, 0));
await probe("promises lchown gid string", () => fsp.lchown("/tmp", 0, "x" as any));
await probe("promises rm options string", () => fsp.rm("/tmp/perry_promises_arg_validation_missing", "x" as any));
await probe("promises rm options null", () => fsp.rm("/tmp/perry_promises_arg_validation_missing", null as any));
await probe("promises truncate path bool", () => fsp.truncate(true as any, 1));
await probe("promises readlink options number", () =>
  fsp.readlink("/tmp/perry_promises_arg_validation_missing", 5 as any),
);
