import { Deserializer, Serializer } from "node:v8";

for (
  const [label, call] of [
    [
      "writeRawBytes string",
      () => new Serializer().writeRawBytes("bad" as any),
    ],
    ["writeRawBytes object", () => new Serializer().writeRawBytes({} as any)],
    [
      "readDouble empty",
      () => new Deserializer(new Serializer().releaseBuffer()).readDouble(),
    ],
    [
      "readRawBytes negative",
      () => new Deserializer(Buffer.alloc(4)).readRawBytes(-1),
    ],
  ] as const
) {
  try {
    call();
    console.log(label + ": no throw");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code ?? "no-code");
  }
}
