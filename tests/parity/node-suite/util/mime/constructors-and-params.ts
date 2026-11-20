import { MIMEParams, MIMEType } from "node:util";

function showInvalid(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ":", "accepted");
  } catch (err) {
    const e = err as NodeJS.ErrnoException;
    console.log(label + ":", e.name, e.code);
  }
}

const mime = new MIMEType('Text/HTML; Charset=UTF-8; boundary="abc def"; x=1');
console.log("basic:", mime.type, mime.subtype, mime.essence, String(mime));
console.log(
  "params:",
  mime.params.get("charset"),
  mime.params.get("boundary"),
  mime.params.has("x"),
  Array.from(mime.params.keys()).join("|"),
  Array.from(mime.params.values()).join("|"),
);

mime.type = "Application";
mime.subtype = "JSON";
mime.params.set("Version", "1.0");
mime.params.delete("x");
console.log("mutated:", mime.type, mime.subtype, mime.essence, mime.toString());
console.log("case:", mime.params.get("version"), mime.params.has("Version"));

const params = new MIMEParams([["ignored", "yes"]]);
params.set("c", "needs;quote");
console.log("standalone:", params.toString(), params.get("ignored"));

showInvalid("bad mime", () => new MIMEType("text"));
showInvalid("bad param name", () => params.set("bad name", "x"));
showInvalid("bad param value", () => params.set("ok", "bad\nvalue"));
