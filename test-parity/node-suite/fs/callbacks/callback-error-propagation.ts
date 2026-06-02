import * as fs from "node:fs";

// Verify that callback-style fs APIs surface real Errors as the first
// argument when the underlying operation fails. Previously Perry always
// passed `err = null` regardless of outcome.
const MISSING = "/tmp/perry_node_suite_definitely_missing_path_xyz123";

await new Promise<void>((resolve) => {
  fs.readFile(MISSING, "utf8", (err, _data) => {
    console.log("readFile missing err is Error:", err instanceof Error);
    console.log("readFile missing err.code:", err && (err as any).code);
    console.log("readFile missing err.syscall:", err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.stat(MISSING, (err, _stats) => {
    console.log("stat missing err is Error:", err instanceof Error);
    console.log("stat missing err.code:", err && (err as any).code);
    console.log("stat missing err.syscall:", err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.unlink(MISSING, (err) => {
    console.log("unlink missing err is Error:", err instanceof Error);
    console.log("unlink missing err.code:", err && (err as any).code);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.access(MISSING, (err) => {
    console.log("access missing err is Error:", err instanceof Error);
    console.log("access missing err.code:", err && (err as any).code);
    resolve();
  });
});
