import v8 from "node:v8";

// deep-equality helper (handles primitives, arrays, plain objects, Buffer-like)
function deepEqual(a: any, b: any): boolean {
  if (a === b) return true;
  if (typeof a !== typeof b) return false;
  if (typeof a === "bigint") return a === b;
  if (a === null || b === null) return a === b;
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b)) return false;
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      if (!deepEqual(a[i], b[i])) return false;
    }
    return true;
  }
  if (typeof a === "object") {
    const ak = Object.keys(a).sort();
    const bk = Object.keys(b).sort();
    if (ak.length !== bk.length) return false;
    for (let i = 0; i < ak.length; i++) {
      if (ak[i] !== bk[i]) return false;
      if (!deepEqual(a[ak[i]], b[bk[i]])) return false;
    }
    return true;
  }
  return false;
}

// ---- #3137: v8.serialize / v8.deserialize round trips ----
const primitives: any[] = [42, -7, 3.5, "hello", "", true, false, null];
for (const value of primitives) {
  const buf = v8.serialize(value);
  const out = v8.deserialize(buf);
  console.log("prim", out === value, typeof out);
}

const obj = { a: 1, b: "x", c: [true, false, null], d: { nested: 9 } };
const objOut = v8.deserialize(v8.serialize(obj));
console.log("object", deepEqual(obj, objOut));

const arr = [1, "two", 3.5, [4, 5], { k: "v" }];
const arrOut = v8.deserialize(v8.serialize(arr));
console.log("array", deepEqual(arr, arrOut));

const big = 9007199254740993n;
const bigOut = v8.deserialize(v8.serialize(big));
console.log("bigint", bigOut === big);

const dt = new Date("2020-01-02T03:04:05.000Z");
const dtOut = v8.deserialize(v8.serialize(dt));
console.log("date", dtOut instanceof Date && dtOut.getTime() === dt.getTime());

console.log("isBuffer", Buffer.isBuffer(v8.serialize(obj)));

// ---- #3138: heap statistics shape ----
const hs = v8.getHeapStatistics();
const heapKeys = [
  "total_heap_size",
  "used_heap_size",
  "heap_size_limit",
  "external_memory",
  "total_available_size",
  "total_physical_size",
  "number_of_native_contexts",
  "number_of_detached_contexts",
  "total_allocated_bytes",
];
console.log(
  "heap",
  heapKeys.every((k) => typeof (hs as any)[k] === "number"),
);
console.log("cachedDataVersionTag", typeof v8.cachedDataVersionTag());

const cs = v8.getHeapCodeStatistics();
console.log("codeStats", typeof cs === "object" && cs !== null);

const ss = v8.getHeapSpaceStatistics();
console.log(
  "spaceStats",
  Array.isArray(ss) &&
    ss.length > 0 &&
    typeof ss[0].space_name === "string" &&
    typeof ss[0].space_size === "number",
);

// ---- #3142: GCProfiler report shape ----
console.log("GCProfiler typeof", typeof v8.GCProfiler);
const profiler = new v8.GCProfiler();
console.log("start typeof", typeof profiler.start);
console.log("stop typeof", typeof profiler.stop);
console.log("start ret", profiler.start());
const report = profiler.stop();
console.log("report keys", JSON.stringify(Object.keys(report).sort()));
console.log(
  "report types",
  typeof report.version === "number" &&
    typeof report.startTime === "number" &&
    typeof report.endTime === "number" &&
    Array.isArray(report.statistics),
);
