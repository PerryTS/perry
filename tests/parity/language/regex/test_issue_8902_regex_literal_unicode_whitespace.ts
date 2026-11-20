// Regression test for #8902: raw Unicode whitespace in a regex literal must
// remain part of the pattern instead of being discarded by source parsing.

function codePoints(value: string): string {
  return [...value]
    .map((character) => character.codePointAt(0)!.toString(16).padStart(4, "0"))
    .join(" ");
}

const nbsp = `x${String.fromCharCode(0x00a0)}y`;
const narrowNbsp = `x${String.fromCharCode(0x202f)}y`;

console.log(codePoints(nbsp.replace(/ /g, " ")));
console.log(codePoints(nbsp.replace(/[ ]/g, " ")));
console.log(codePoints(narrowNbsp.replace(/ /g, " ")));
console.log(codePoints(narrowNbsp.replace(/[ ]/g, " ")));

// Controls: escaped literals and constructor patterns already worked.
console.log(codePoints(nbsp.replace(/\u00a0/g, " ")));
console.log(codePoints(nbsp.replace(new RegExp("[\\u00a0]", "g"), " ")));
