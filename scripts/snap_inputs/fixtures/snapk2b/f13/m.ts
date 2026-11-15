// An Error made two calls deep in a recursive JS function at module scope:
// the capture holds those function frames (a module init body is not a
// JS-text frame), and the classifier sorts their pcs.
function make(depth: number): Error {
  return depth > 0 ? make(depth - 1) : new Error("k2b");
}
export const e = make(2);
