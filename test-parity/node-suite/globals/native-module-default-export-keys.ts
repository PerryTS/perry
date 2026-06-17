// #3906: default/namespace imports of native node builtins must enumerate the
// module's real export surface, not the internal `__module__` sentinel.
import tty from "node:tty";
import v8 from "node:v8";
import perfHooks from "node:perf_hooks";
import utilTypes from "node:util/types";

// The internal sentinel must never surface as an enumerable own key.
for (const [name, mod] of [
  ["tty", tty],
  ["v8", v8],
  ["perf_hooks", perfHooks],
  ["util/types", utilTypes],
] as const) {
  console.log(name, "has __module__:", Object.keys(mod).includes("__module__"));
}

// Modules Perry implements end-to-end enumerate byte-for-byte like Node.
console.log("tty:", Object.keys(tty).join(","));
console.log("perf_hooks:", Object.keys(perfHooks).join(","));
console.log("util/types:", Object.keys(utilTypes).join(","));

// Representative v8 exports are enumerable (Perry exposes its supported subset).
for (const k of [
  "serialize",
  "deserialize",
  "Serializer",
  "getHeapSnapshot",
  "getHeapStatistics",
  "cachedDataVersionTag",
  "writeHeapSnapshot",
]) {
  console.log("v8 has", k + ":", Object.keys(v8).includes(k));
}

// `Object.keys` and `hasOwnProperty` agree on the enumerated surface.
console.log("hasOwnProperty agrees:", Object.keys(utilTypes).every((k) => Object.prototype.hasOwnProperty.call(utilTypes, k)));

// Every enumerated v8 key resolves to a value via dynamic read (no `undefined`
// holes), matching Node's `typeof` for each.
const v8Keys = [
  "cachedDataVersionTag", "getHeapSnapshot", "getHeapStatistics", "getHeapSpaceStatistics", "getHeapCodeStatistics",
  "setFlagsFromString", "Serializer", "Deserializer", "DefaultSerializer", "DefaultDeserializer",
  "deserialize", "takeCoverage", "stopCoverage", "serialize", "writeHeapSnapshot", "promiseHooks", "startupSnapshot",
  "setHeapSnapshotNearHeapLimit", "GCProfiler",
];
for (const k of v8Keys) {
  console.log("typeof v8." + k + ":", typeof (v8 as any)[k]);
}

// A captured/aliased v8 helper invokes its real implementation.
const serialize = (v8 as any).serialize;
const deserialize = (v8 as any).deserialize;
console.log("v8 serialize roundtrip:", JSON.stringify(deserialize(serialize({ a: 1, b: [2, 3] }))));
console.log("v8 cachedDataVersionTag bound typeof:", typeof (v8 as any).cachedDataVersionTag());
