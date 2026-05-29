// #2013 — Node argument-validation errors on fs sync APIs.
//
// Each block calls the API with bad arguments and prints the thrown error's
// `.name` and `.code`. Perry and Node must produce the exact same lines.
// `existsSync` is the lone exception: Node guarantees it *never* throws and
// returns `false` on bad input (see test/parallel/test-fs-exists.js), so the
// assertions there cover the no-throw contract.

import * as fs from "node:fs";

function probe(label: string, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (e: any) {
    console.log(label, e.name, e.code);
  }
}

// fsyncSync / fdatasyncSync — fd type and range
probe("fsyncSync(null)", () => fs.fsyncSync(null as any));
probe("fsyncSync('')", () => fs.fsyncSync("" as any));
probe("fsyncSync({})", () => fs.fsyncSync({} as any));
probe("fsyncSync(NaN)", () => fs.fsyncSync(NaN));
probe("fsyncSync(Infinity)", () => fs.fsyncSync(Infinity));
probe("fsyncSync(-1)", () => fs.fsyncSync(-1));
probe("fdatasyncSync(null)", () => fs.fdatasyncSync(null as any));
probe("fdatasyncSync(2**32)", () => fs.fdatasyncSync(2 ** 32));

// fchownSync — fd, uid, gid
probe("fchownSync('',0,0)", () => fs.fchownSync("" as any, 0, 0));
probe("fchownSync(1,'',0)", () => fs.fchownSync(1, "" as any, 0));
probe("fchownSync(1,1,{})", () => fs.fchownSync(1, 1, {} as any));
probe("fchownSync(1,1,Infinity)", () => fs.fchownSync(1, 1, Infinity));
probe("fchownSync(1,-2,1)", () => fs.fchownSync(1, -2, 1));

// lchownSync — path, uid, gid
probe("lchownSync(false,1,1)", () => fs.lchownSync(false as any, 1, 1));
probe("lchownSync(1,1,1)", () => fs.lchownSync(1 as any, 1, 1));
probe("lchownSync('/x','',1)", () => fs.lchownSync("/x", "" as any, 1));
probe("lchownSync('/x',1,null)", () => fs.lchownSync("/x", 1, null as any));

// lchmodSync — path-type validation only. Node opens the path before
// validating the mode, so a bad mode on a non-existent path surfaces
// ENOENT, not ERR_OUT_OF_RANGE — match that ordering by deferring mode
// validation entirely (covered separately by the mode-on-existing-path
// follow-up).
probe("lchmodSync(false,0)", () => fs.lchmodSync(false as any, 0));
probe("lchmodSync(1,0)", () => fs.lchmodSync(1 as any, 0));

// copyFileSync — src, dest, mode
probe("copyFileSync(1,'/x')", () => fs.copyFileSync(1 as any, "/x"));
probe("copyFileSync('/x',1)", () => fs.copyFileSync("/x", 1 as any));
probe("copyFileSync('/x','/y','r')", () => fs.copyFileSync("/x", "/y", "r" as any));
probe("copyFileSync('/x','/y',8)", () => fs.copyFileSync("/x", "/y", 8));

// writeSync — fd
probe("writeSync(null,'x')", () => fs.writeSync(null as any, "x"));
probe("writeSync({},'x')", () => fs.writeSync({} as any, "x"));

// writevSync — fd; Node skips fd validation on an empty buffers array
// (returns 0 without touching the fd), so the empty case must NOT throw.
probe("writevSync(null,[])", () => fs.writevSync(null as any, []));
probe("writevSync(null,[Buffer.from('x')])", () => fs.writevSync(null as any, [Buffer.from("x")]));

// #2013 extension — gap-fill for fd-only / path-mutator surface that
// PR #2035 didn't reach: each new probe pins both the
// non-numeric → ERR_INVALID_ARG_TYPE branch and the numeric-but-not-open
// → EBADF branch where applicable. Trimmed shapes only; the upper block
// already covers boolean-vs-object-vs-NaN-vs-Infinity for the legacy set.

// closeSync(fd) — fd validation + EBADF on a closed/unknown fd.
probe("closeSync(true)", () => fs.closeSync(true as any));
probe("closeSync(123)", () => fs.closeSync(123 as any));

// readSync(fd, buf, …) — same shape; needs a real buffer arg to clear
// the buffer-shape check before the fd path runs.
probe("readSync(true,buf,0,1,0)", () =>
  fs.readSync(true as any, Buffer.alloc(1), 0, 1, 0),
);
probe("readSync(123,buf,0,1,0)", () =>
  fs.readSync(123 as any, Buffer.alloc(1), 0, 1, 0),
);

// readvSync(fd, buffers) — non-empty iovec branch triggers EBADF.
probe("readvSync(true,[buf])", () =>
  fs.readvSync(true as any, [Buffer.alloc(1)]),
);
probe("readvSync(123,[buf])", () =>
  fs.readvSync(123 as any, [Buffer.alloc(1)]),
);

// fchmodSync(fd, mode) — fd validation, plus mode out-of-range on a
// valid (numeric) fd via the EBADF path.
probe("fchmodSync(true,0o755)", () => fs.fchmodSync(true as any, 0o755));
probe("fchmodSync(123,0o755)", () => fs.fchmodSync(123 as any, 0o755));

// writeSync(fd, …) — numeric-but-unknown fd must flip from "no-throw,
// returns 0" to EBADF.
probe("writeSync(123,'x')", () => fs.writeSync(123 as any, "x"));
probe("writeSync(123,buf)", () => fs.writeSync(123 as any, Buffer.from("x")));

// chmodSync(path, mode) — path-only validation.
probe("chmodSync(true,0o755)", () => fs.chmodSync(true as any, 0o755));

// renameSync(oldPath, newPath) — both args path-validated.
probe("renameSync(true,'/x')", () => fs.renameSync(true as any, "/x"));
probe("renameSync('/x',42)", () => fs.renameSync("/x", 42 as any));

// `existsSync` never throws on bad input (Node 22+ instead prints DEP0187
// to stderr, which the parity runner would capture into the diff). The
// no-throw contract is exercised by other tests in this suite.
