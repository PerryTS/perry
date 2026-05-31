import { StringDecoder } from "node:string_decoder";

function probe(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "OK", JSON.stringify(value));
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

probe("ctor bogus", () => new StringDecoder("bogus"));
probe("write missing", () => new StringDecoder("utf8").write(undefined as any));
probe("write null", () => new StringDecoder("utf8").write(null as any));
probe("write number", () => new StringDecoder("utf8").write(1 as any));
probe("write object", () => new StringDecoder("utf8").write({} as any));
probe("write string", () => new StringDecoder("utf8").write("abc" as any));
probe("write dataview", () =>
  new StringDecoder("utf8").write(new DataView(new Uint8Array([0x61]).buffer) as any),
);
probe("write arraybuffer", () => new StringDecoder("utf8").write(new ArrayBuffer(1) as any));
probe("end omitted", () => new StringDecoder("utf8").end());
probe("end undefined", () => new StringDecoder("utf8").end(undefined as any));
probe("end null", () => new StringDecoder("utf8").end(null as any));
probe("end string", () => new StringDecoder("utf8").end("abc" as any));
