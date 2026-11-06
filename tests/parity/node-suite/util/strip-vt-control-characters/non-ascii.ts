import { stripVTControlCharacters } from "node:util";
// Regression: byte-by-byte processing previously mangled multi-byte
// UTF-8 sequences when scrubbing escapes (e.g. café -> cafÃ©).
console.log(stripVTControlCharacters("café [31mrojo[39m"));
console.log(stripVTControlCharacters("[1mnaïve[0m déjà vu"));
console.log(stripVTControlCharacters("emoji: [32m\u{1F389}[0m ok"));
