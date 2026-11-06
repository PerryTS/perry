import { Blob, resolveObjectURL } from "node:buffer";

function marker(message: string, needle: string, present: string, missing: string): string {
  return message.indexOf(needle) >= 0 ? present : missing;
}

function report(label: string, fn: () => unknown): void {
  try {
    const value = fn();
    console.log(label, "ok", value === undefined ? "undefined" : String(value));
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
        'The "obj" argument must be an instance of Blob',
        "obj-msg",
        "no-obj-msg",
      ),
      marker(message, "Received type string ('x')", "string-received", "no-string-received"),
      marker(message, "Received an instance of Object", "object-received", "no-object-received"),
    );
  }
}

report("create string", () => URL.createObjectURL("x" as any));
report("create object", () => URL.createObjectURL({} as any));

const blob = new Blob(["hi"], { type: "text/plain" });
const id = URL.createObjectURL(blob);
const uuidShape =
  /^blob:nodedata:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id);
console.log("id shape", uuidShape);

const resolved = resolveObjectURL(id);
console.log("resolved", resolved && resolved.size, resolved && resolved.type);
console.log("resolve bad", resolveObjectURL(123 as any));
console.log("revoke bad", URL.revokeObjectURL(123 as any));
URL.revokeObjectURL(id);
console.log("after revoke", resolveObjectURL(id));
