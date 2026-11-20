import { transcode, Buffer } from "node:buffer";

function marker(message: string, needle: string, present: string, missing: string): string {
  return message.indexOf(needle) >= 0 ? present : missing;
}

function report(label: string, fn: () => Buffer): void {
  try {
    const value = fn();
    console.log(label, "ok", Buffer.isBuffer(value), value.toString("hex"));
  } catch (err) {
    const e = err as any;
    const message = String(e && e.message);
    console.log(
      label,
      "throw",
      e && e.name,
      e && e.code,
      marker(
        message,
        "Unable to transcode Buffer [U_ILLEGAL_ARGUMENT_ERROR]",
        "transcode-msg",
        "no-transcode-msg",
      ),
      marker(
        message,
        'The "source" argument must be an instance of Buffer or Uint8Array',
        "source-msg",
        "no-source-msg",
      ),
      marker(message, "Received type string ('hi')", "string-received", "no-string-received"),
      marker(
        message,
        "Received an instance of ArrayBuffer",
        "arraybuffer-received",
        "no-arraybuffer-received",
      ),
    );
  }
}

report("buffer utf8 latin1", () => transcode(Buffer.from("hi"), "utf8", "latin1"));
report("buffer latin1 utf8", () => transcode(Buffer.from([0xff]), "latin1", "utf8"));
report("uint8 utf8 latin1", () => transcode(new Uint8Array([0x68, 0x69]), "utf8", "latin1"));
report("bad from encoding", () => transcode(Buffer.from("hi"), "bad", "utf8"));
report("bad to encoding", () => transcode(Buffer.from("hi"), "utf8", "bad"));
report("unsupported encoding", () => transcode(Buffer.from("hi"), "hex", "utf8"));
report("non-string encoding", () => transcode(Buffer.from("hi"), 1 as any, "utf8"));
report("string source", () => transcode("hi" as any, "utf8", "latin1"));
report("arraybuffer source", () => transcode(new ArrayBuffer(2) as any, "utf8", "latin1"));
