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

await new Promise<void>((resolve) => {
  fs.ftruncate(BAD_FD, 0, (err) => {
    console.log("ftruncate", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.fchown(BAD_FD, 0, 0, (err) => {
    console.log("fchown", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.futimes(BAD_FD, 1, 1, (err) => {
    console.log("futimes", err && (err as any).code, err && (err as any).syscall);
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.fstat(BAD_FD, (err, stats) => {
    console.log("fstat", err && (err as any).code, err && (err as any).syscall, String(stats === undefined));
    resolve();
  });
});

const buf = Buffer.alloc(4);
const bufs = [Buffer.alloc(2)];

await new Promise<void>((resolve) => {
  fs.read(BAD_FD, buf, 0, 1, 0, (err, bytesRead, buffer) => {
    console.log("read", err && (err as any).code, err && (err as any).syscall, String(bytesRead), String(buffer === buf));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.read(BAD_FD, buf, { offset: 0, length: 1, position: 0 }, (err, bytesRead, buffer) => {
    console.log("read-options", err && (err as any).code, err && (err as any).syscall, String(bytesRead), String(buffer === buf));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.write(BAD_FD, "x", (err, bytesWritten, data) => {
    console.log("write-string", err && (err as any).code, err && (err as any).syscall, String(bytesWritten), String(data));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.write(BAD_FD, buf, 0, 1, 0, (err, bytesWritten, buffer) => {
    console.log("write-buffer", err && (err as any).code, err && (err as any).syscall, String(bytesWritten), String(buffer === buf));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.write(BAD_FD, buf, { offset: 0, length: 1, position: 0 }, (err, bytesWritten, buffer) => {
    console.log("write-options", err && (err as any).code, err && (err as any).syscall, String(bytesWritten), String(buffer === buf));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.readv(BAD_FD, bufs, 0, (err, bytesRead, buffers) => {
    console.log("readv", err && (err as any).code, err && (err as any).syscall, String(bytesRead), String(buffers === bufs));
    resolve();
  });
});

await new Promise<void>((resolve) => {
  fs.writev(BAD_FD, bufs, 0, (err, bytesWritten, buffers) => {
    console.log("writev", err && (err as any).code, err && (err as any).syscall, String(bytesWritten), String(buffers === bufs));
    resolve();
  });
});
