// `stream.duplexPair([opts])` returns a two-element array of paired
// Duplex streams. Perry's stubs return a pair of fresh Duplex stubs
// (cross-stream piping isn't propagated yet); the test only asserts
// shape (length 2 + both entries are objects). Regression cover for
// #1539.
import { duplexPair } from "node:stream";
const pair = duplexPair();
console.log("length:", pair.length);
console.log("[0] typeof:", typeof pair[0]);
console.log("[1] typeof:", typeof pair[1]);
