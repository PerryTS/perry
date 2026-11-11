// #679 Phase 4 — ink source-compat test #3: useStdout terminal dims.
//
// Validates that useStdout() exposes columns/rows. ink programs use
// these to fit a layout to the terminal width (e.g. log viewer that
// truncates rows beyond `columns - 2`).

import { useStdout } from "perry/tui";

const stdout = useStdout();
const c = stdout.columns();
const r = stdout.rows();
console.log("COLS_POSITIVE=" + (c >= 1));
console.log("ROWS_POSITIVE=" + (r >= 1));
console.log("DONE");
