import * as v8 from "node:v8";

for (
  const [label, fn] of [
    ["Serializer.writeHeader", v8.Serializer.prototype.writeHeader],
    ["Serializer.releaseBuffer", v8.Serializer.prototype.releaseBuffer],
    ["Deserializer.readHeader", v8.Deserializer.prototype.readHeader],
    ["GCProfiler.start", v8.GCProfiler.prototype.start],
  ] as const
) {
  for (
    const [receiverLabel, receiver] of [["undefined", undefined], [
      "object",
      {},
    ], ["null", null]] as const
  ) {
    try {
      fn.call(receiver);
      console.log(label + " " + receiverLabel + ": no throw");
    } catch (error: any) {
      console.log(
        label + " " + receiverLabel + ":",
        error.name,
        error.code ?? "no-code",
      );
    }
  }
}
