import { Readable } from "node:stream";
// readable[Symbol.asyncIterator]() returns an object with next/return methods.
const r = Readable.from(["a"]);
const it = (r as any)[Symbol.asyncIterator]();
console.log("has next:", typeof it.next === "function");
console.log("has return:", typeof it.return === "function");
console.log("self-iterable:", typeof it[Symbol.asyncIterator] === "function");
console.log("self iterates to same:", it[Symbol.asyncIterator]() === it);
