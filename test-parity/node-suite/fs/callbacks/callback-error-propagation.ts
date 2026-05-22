import * as fs from "node:fs";

// Verify that callback-style fs APIs surface real Errors as the first
// argument when the underlying operation fails. Previously Perry always
// passed `err = null` regardless of outcome.
const MISSING = "/tmp/perry_node_suite_definitely_missing_path_xyz123";

// Note: we assert `err instanceof Error` plus an ENOENT substring in the
// message rather than `err.code === "ENOENT"`. Perry's Error object stores
// `code`/`syscall`/`path` in a side table that isn't yet wired into property
// reads (tracked under STATUS.md item 12); the message text contains the
// code so user code that branches on it still works via includes/regex.
const hasEnoent = (err: unknown) =>
  err instanceof Error && /ENOENT/.test(err.message);

await new Promise<void>((resolve) => {
  fs.readFile(MISSING, "utf8", (err, _data) => {
    console.log("readFile missing err is Error:", err instanceof Error);
    console.log("readFile missing err mentions ENOENT:", hasEnoent(err));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.stat(MISSING, (err, _stats) => {
    console.log("stat missing err is Error:", err instanceof Error);
    console.log("stat missing err mentions ENOENT:", hasEnoent(err));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.unlink(MISSING, (err) => {
    console.log("unlink missing err is Error:", err instanceof Error);
    console.log("unlink missing err mentions ENOENT:", hasEnoent(err));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.access(MISSING, (err) => {
    console.log("access missing err is Error:", err instanceof Error);
    console.log("access missing err mentions ENOENT:", hasEnoent(err));
    resolve();
  });
});
