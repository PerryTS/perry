// A for-of over an array literal: the record glue arrayRecordLiteral(...) is a fresh Array of exactly its arguments.
function squareIndex(square: string): number {
  if (square.length !== 2) throw new Error("bad " + square);
  return (square.charCodeAt(1) - 49) * 16 + square.charCodeAt(0) - 97;
}
let total = 0;
for (const sq of ["e2", "a7", "h8"]) total += squareIndex(sq);
console.log(total);
