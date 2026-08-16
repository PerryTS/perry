// #8002/#8003: realm intrinsics, long-lived native-module values and Web
// Storage brand pointers must belong to the calling `perry/thread` agent.
//
// Warm every affected family on the main thread first. Before the fix that
// planted raw main-arena addresses in process-global caches; worker access then
// reused or overwrote them. The worker mutates the observable realm towers,
// drives for-of and a generator under forced collections, reads the cached
// native-module values, and uses both Storage brands. Its mutations must stay
// invisible to the main realm after it exits.
import { spawn } from "perry/thread";

declare function require(name: string): any;

type Tagged = { realmOwner?: string };

function* syncGenerator(): Generator<number, void, unknown> {
  yield 4;
  yield 5;
  yield 6;
}

function warmAndSummarize(tag: string): string {
  const iteratorProto = Object.getPrototypeOf([1][Symbol.iterator]()) as Tagged;
  const generatorProto = Object.getPrototypeOf(Object.getPrototypeOf(syncGenerator())) as Tagged;
  const typedArrayProto = Object.getPrototypeOf(Uint8Array.prototype) as Tagged;
  iteratorProto.realmOwner = tag + "-iterator";
  generatorProto.realmOwner = tag + "-generator";
  typedArrayProto.realmOwner = tag + "-typedarray";

  let total = 0;
  for (const value of [1, 2, 3]) total += value;
  for (const value of syncGenerator()) total += value;
  const typed = new Uint8Array([7, 8, 9]);
  for (const value of typed) total += value;

  // Keep the affected roots live across enough short-lived allocations to
  // force nursery turnover in both realms. The checksum makes this pressure
  // observable, so neither the objects nor their reads can be discarded.
  let pressure = 0;
  for (let i = 0; i < 30000; i++) {
    const row = { a: i, b: i + 1, c: i + 2 };
    const values = [row.a, row.b, row.c];
    pressure = (pressure + values[0] + values[1] + values[2]) % 1000003;
  }

  const fs = require("node:fs");
  const os = require("node:os");
  const native = [
    typeof fs.constants.O_RDONLY === "number",
    typeof os.constants.signals.SIGINT === "number",
    typeof os.constants.errno.EACCES === "number",
    typeof os.constants.priority.PRIORITY_NORMAL === "number",
    typeof os.constants.dlopen.RTLD_LAZY === "number",
  ].every(Boolean);

  localStorage.clear();
  sessionStorage.clear();
  localStorage.setItem("owner", tag + "-local");
  sessionStorage.setItem("owner", tag + "-session");
  const storage = localStorage.getItem("owner") + "/" + sessionStorage.getItem("owner");

  return [
    iteratorProto.realmOwner,
    generatorProto.realmOwner,
    typedArrayProto.realmOwner,
    String(total),
    String(pressure),
    String(native),
    storage,
  ].join("|");
}

const main = warmAndSummarize("main");
console.log("main:", main);

const worker = await spawn((): string => warmAndSummarize("worker"));
const expectedWorker =
  "worker-iterator|worker-generator|worker-typedarray|45|40950|true|worker-local/worker-session";
console.log("worker:", worker, "match:", worker === expectedWorker);

const mainIteratorProto = Object.getPrototypeOf([1][Symbol.iterator]()) as Tagged;
const mainGeneratorProto = Object.getPrototypeOf(Object.getPrototypeOf(syncGenerator())) as Tagged;
const mainTypedArrayProto = Object.getPrototypeOf(Uint8Array.prototype) as Tagged;
console.log(
  "main after:",
  [
    mainIteratorProto.realmOwner,
    mainGeneratorProto.realmOwner,
    mainTypedArrayProto.realmOwner,
    localStorage.getItem("owner"),
    sessionStorage.getItem("owner"),
  ].join("|"),
);
