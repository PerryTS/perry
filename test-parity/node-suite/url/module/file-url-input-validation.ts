import { fileURLToPath, fileURLToPathBuffer, pathToFileURL } from "node:url";

function show(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label + ":", String(value));
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code", err?.message);
  }
}

const invalidFileInputs = [
  ["number", 123],
  ["null", null],
  ["buffer", Buffer.from("file:///tmp/a")],
  ["plain-object", { href: "file:///tmp/a" }],
] as any[];

for (const [label, value] of invalidFileInputs) {
  show("fileURLToPath " + label, () => fileURLToPath(value));
}

for (const [label, value] of invalidFileInputs) {
  show("fileURLToPathBuffer " + label, () => fileURLToPathBuffer(value));
}

console.log("fileURLToPath URL:", fileURLToPath(new URL("file:///tmp/a%20b")));
console.log("fileURLToPathBuffer bytes:", fileURLToPathBuffer("file:///tmp/a%FF").toString("hex"));

for (const [label, value] of [
  ["number", 123],
  ["buffer", Buffer.from("/tmp/a")],
  ["symbol", Symbol("x")],
] as any[]) {
  show("pathToFileURL " + label, () => pathToFileURL(value));
}

console.log("pathToFileURL valid:", pathToFileURL("/tmp/a b").href);
