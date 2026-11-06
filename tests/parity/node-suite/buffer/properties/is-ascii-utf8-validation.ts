import { Buffer, isAscii, isUtf8 } from "node:buffer";

type PredicateName = "isAscii" | "isUtf8";

function runPredicate(fnName: PredicateName, value: any): boolean {
  if (fnName === "isAscii") return isAscii(value);
  return isUtf8(value);
}

function show(fnName: PredicateName, label: string, value: any): void {
  try {
    console.log(label, fnName, "ok", runPredicate(fnName, value));
  } catch (err) {
    const e = err as Error & { code?: string };
    console.log(
      label,
      fnName,
      "throw",
      e.name,
      e.code ?? "no-code",
      e.message.includes('"input"'),
      err instanceof TypeError,
    );
  }
}

for (const [label, value] of [
  ["string", "x"],
  ["number", 1],
  ["null", null],
  ["undefined", undefined],
  ["object", {}],
  ["array", []],
  ["dataview", new DataView(new ArrayBuffer(1))],
] as const) {
  show("isAscii", label, value);
  show("isUtf8", label, value);
}

console.log("buffer", isAscii(Buffer.from("hi")), isUtf8(Buffer.from("hé")));
console.log("uint8", isAscii(new Uint8Array([0x41])), isUtf8(new Uint8Array([0xff])));

const arrayBuffer = new Uint8Array([0xff]).buffer;
console.log("arraybuffer", isAscii(arrayBuffer), isUtf8(arrayBuffer));
