import * as v8 from "node:v8";

for (
  const name of [
    "Serializer",
    "Deserializer",
    "DefaultSerializer",
    "DefaultDeserializer",
    "GCProfiler",
  ] as const
) {
  try {
    (v8[name] as any)();
    console.log(name + ": no throw");
  } catch (error: any) {
    console.log(name + ":", error.name, error.code);
  }
}

for (
  const [label, value] of [["undefined", undefined], ["string", "bad"], [
    "object",
    {},
  ]] as const
) {
  try {
    new v8.Deserializer(value as any);
    console.log("Deserializer " + label + ": no throw");
  } catch (error: any) {
    console.log("Deserializer " + label + ":", error.name, error.code);
  }
}
