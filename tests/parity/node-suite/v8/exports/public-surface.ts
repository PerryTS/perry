import * as v8 from "node:v8";

const expected = [
  "DefaultDeserializer",
  "DefaultSerializer",
  "Deserializer",
  "GCProfiler",
  "Serializer",
  "cachedDataVersionTag",
  "default",
  "deserialize",
  "getCppHeapStatistics",
  "getHeapCodeStatistics",
  "getHeapSnapshot",
  "getHeapSpaceStatistics",
  "getHeapStatistics",
  "isStringOneByteRepresentation",
  "promiseHooks",
  "queryObjects",
  "serialize",
  "setFlagsFromString",
  "setHeapSnapshotNearHeapLimit",
  "startCpuProfile",
  "startHeapProfile",
  "startupSnapshot",
  "stopCoverage",
  "takeCoverage",
  "writeHeapSnapshot",
];

console.log("keys:", Object.keys(v8).join(","));
console.log("exact:", expected.every((key, i) => Object.keys(v8)[i] === key));
console.log("count:", Object.keys(v8).length);
