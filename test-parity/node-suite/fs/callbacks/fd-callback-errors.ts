import * as fs from "node:fs";

// #3332: callback-style fd helpers (close/fsync/fdatasync/fchmod) must
// DELIVER the EBADF error to the callback for a bad descriptor rather
// than calling the success path. The sync forms throw EBADF, but the
// callback forms report it through the first callback argument.
const BAD_FD = 987654321;

await new Promise<void>((resolve) => {
  fs.close(BAD_FD, (err) => {
    console.log("close", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.fsync(BAD_FD, (err) => {
    console.log("fsync", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.fdatasync(BAD_FD, (err) => {
    console.log("fdatasync", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.fchmod(BAD_FD, 0o600, (err) => {
    console.log("fchmod", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});
