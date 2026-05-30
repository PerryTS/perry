// Issue #2921 — `fs.statfsSync(path)` validates `path` and surfaces
// filesystem errors instead of returning a zero-filled StatFs object. An
// invalid path type throws `TypeError [ERR_INVALID_ARG_TYPE]`; a missing
// path throws `Error [ENOENT]` with syscall `statfs`.
import * as fs from "node:fs";

// Mirror the established fs error-shape probes (stat-lstat-missing-errors.ts):
// assert name/code/syscall, not the raw message body — Perry's `std::io::Error`
// Display divergence ("(os error N)", capitalization) is a pre-existing
// fs-wide formatting gap, orthogonal to throwing-vs-fake-stats here.
function probe(label: string, expectedPath: string | null, fn: () => any) {
  try {
    fn();
    console.log(label, "no-throw");
  } catch (err: any) {
    console.log(
      label,
      err.name,
      err.code,
      err.syscall,
      expectedPath === null ? "" : err.path === expectedPath,
    );
  }
}

const MISSING = "/tmp/__perry_missing_statfs_target__";
probe("statfsSync missing", MISSING, () => fs.statfsSync(MISSING));
probe("statfsSync number", null, () => fs.statfsSync(123 as any));
probe("statfsSync null", null, () => fs.statfsSync(null as any));
probe("statfsSync object", null, () => fs.statfsSync({} as any));
probe("statfsSync boolean", null, () => fs.statfsSync(true as any));

// A valid path still returns a populated StatFs object (regression guard for
// the success path / #2561 fields).
const ok = fs.statfsSync("/tmp");
console.log("statfsSync ok:", typeof ok.bsize, ok.bsize > 0, ok.blocks >= ok.bfree);
