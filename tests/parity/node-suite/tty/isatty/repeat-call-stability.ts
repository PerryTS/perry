import * as tty from "node:tty";

// isatty must be deterministic for a given fd within a single process — the
// answer doesn't change call-to-call, and the function itself remains the same
// reference across reads.
const ref1 = tty.isatty;
const ref2 = tty.isatty;
console.log("isatty self-identity:", ref1 === ref2);

const a = tty.isatty(1);
const b = tty.isatty(1);
const c = tty.isatty(1);
console.log("fd1 stable:", a === b && b === c);

const x = tty.isatty(99);
const y = tty.isatty(99);
console.log("fd99 stable false:", x === false && y === false);
