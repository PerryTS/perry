import * as v8 from "node:v8";
import { Buffer } from "node:buffer";

function errorCode(err: unknown): string {
  const anyErr = err as { code?: string };
  return typeof anyErr.code === "string" ? anyErr.code : "no-code";
}

const heapKeys = [
  "total_heap_size",
  "used_heap_size",
  "heap_size_limit",
  "external_memory",
  "number_of_native_contexts",
  "total_available_size",
  "total_physical_size",
];
const codeKeys = [
  "code_and_metadata_size",
  "bytecode_and_metadata_size",
  "external_script_source_size",
  "cpu_profiler_metadata_size",
];
const spaceKeys = [
  "space_name",
  "space_size",
  "space_used_size",
  "space_available_size",
  "physical_space_size",
];

console.log(
  "namespace types:",
  typeof v8.serialize,
  typeof v8.deserialize,
  typeof v8.Serializer,
  typeof v8.DefaultSerializer,
);
console.log("cached tag type:", typeof v8.cachedDataVersionTag());

const heapStats = v8.getHeapStatistics("ignored" as never);
console.log(
  "heap stat types:",
  heapKeys.map((key) => `${key}:${typeof (heapStats as any)[key]}`).join(","),
);

const codeStats = v8.getHeapCodeStatistics(1 as never, 2 as never);
console.log(
  "code stat types:",
  codeKeys.map((key) => `${key}:${typeof (codeStats as any)[key]}`).join(","),
);

const spaces = v8.getHeapSpaceStatistics("ignored" as never);
console.log(
  "space stats shape:",
  Array.isArray(spaces),
  spaces.length > 0,
  spaceKeys.map((key) => `${key}:${typeof (spaces[0] as any)[key]}`).join(","),
);

const roundInputs: Array<[string, unknown]> = [
  ["object", { a: 1, b: "x", c: [true, null] }],
  ["map", new Map([["a", 1]])],
  ["set", new Set([1, 2])],
  ["bigint", 123n],
  ["buffer", Buffer.from("abc")],
  ["date", new Date("2020-01-02T03:04:05Z")],
  ["undefined", undefined],
];

for (const [name, value] of roundInputs) {
  const buf = v8.serialize(value);
  const out = v8.deserialize(buf);
  const detail =
    out instanceof Map
      ? out.get("a")
      : out instanceof Set
        ? Array.from(out).join("|")
        : Buffer.isBuffer(out)
          ? out.toString("utf8")
          : out instanceof Date
            ? out.toISOString()
            : typeof out === "object" && out
              ? JSON.stringify(out)
              : String(out);
  console.log("round:", name, Buffer.isBuffer(buf), detail);
}

for (const value of [() => 1, Symbol("x"), new WeakMap()]) {
  try {
    v8.serialize(value);
    console.log("serialize error:", "none");
  } catch (err) {
    console.log("serialize error:", (err as Error).name, errorCode(err));
  }
}

for (const value of [undefined, null, 1, {}, []]) {
  try {
    v8.deserialize(value as never);
    console.log("deserialize error:", "none");
  } catch (err) {
    console.log("deserialize error:", (err as Error).name, errorCode(err));
  }
}

const serializer = new v8.Serializer();
console.log(
  "serializer methods:",
  typeof serializer.writeHeader,
  typeof serializer.writeValue,
  typeof serializer.releaseBuffer,
);
console.log("serializer write:", serializer.writeHeader(), serializer.writeValue({ a: 1 }));
const serialized = serializer.releaseBuffer();
console.log("serializer buffer:", Buffer.isBuffer(serialized), serialized.length > 0);

const deserializer = new v8.Deserializer(serialized);
console.log(
  "deserializer methods:",
  typeof deserializer.readHeader,
  typeof deserializer.readValue,
  typeof deserializer.getWireFormatVersion,
);
console.log(
  "deserializer read:",
  deserializer.readHeader(),
  deserializer.readValue().a,
  typeof deserializer.getWireFormatVersion(),
);

const defaultSerializer = new v8.DefaultSerializer();
defaultSerializer.writeHeader();
defaultSerializer.writeValue(Buffer.from("xy"));
const defaultDeserializer = new v8.DefaultDeserializer(defaultSerializer.releaseBuffer());
defaultDeserializer.readHeader();
console.log("default classes:", Buffer.isBuffer(defaultDeserializer.readValue()));

try {
  new v8.Deserializer({} as never);
  console.log("deserializer ctor error:", "none");
} catch (err) {
  console.log("deserializer ctor error:", (err as Error).name, errorCode(err));
}
