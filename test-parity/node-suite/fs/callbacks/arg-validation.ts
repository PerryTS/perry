import * as fs from "node:fs";

function probe(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(label, "name", err.name);
    console.log(label, "code", err.code);
    console.log(label, "message", err.message);
  }
}

probe("readFile missing cb", () => fs.readFile("/tmp/perry_callback_arg_validation_missing"));
probe("readFile bad options no cb", () => fs.readFile("/tmp/perry_callback_arg_validation_missing", 5 as any, 0 as any));
probe("readFile truthy bad cb", () => fs.readFile("/tmp/perry_callback_arg_validation_missing", "utf8", {} as any));
probe("writeFile nonfunction cb", () => fs.writeFile("/tmp/perry_callback_arg_validation_write", "x", 0 as any));
probe("mkdir options missing cb", () => fs.mkdir("/tmp/perry_callback_arg_validation_dir", {}));
probe("readdir options missing cb", () => fs.readdir("/tmp", 0 as any));
probe("access bad path missing cb", () => fs.access(true as any, 0 as any));
probe("access bad path bad mode with cb", () => fs.access(true as any, "x" as any, () => {}));
probe("access bad mode with cb", () => fs.access("/tmp", "x" as any, () => {}));
probe("copyFile nonfunction cb", () =>
  fs.copyFile("/tmp/perry_callback_arg_validation_a", "/tmp/perry_callback_arg_validation_b", 0, 0 as any),
);
probe("copyFile bad mode with cb", () =>
  fs.copyFile("/tmp/perry_callback_arg_validation_a", "/tmp/perry_callback_arg_validation_b", "x" as any, () => {}),
);
probe("symlink type missing cb", () =>
  fs.symlink("/tmp/perry_callback_arg_validation_a", "/tmp/perry_callback_arg_validation_b", "file"),
);
probe("fstat fd type", () => fs.fstat("" as any, () => {}));
probe("fsync fd type", () => fs.fsync("" as any, () => {}));
