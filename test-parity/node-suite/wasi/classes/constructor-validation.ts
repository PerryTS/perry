import wasiDefault from "node:wasi";
import { WASI } from "node:wasi";

const W: any = WASI;

function check(label: string, fn: () => any) {
  try {
    const value = fn();
    console.log(
      label + ": ok",
      Object.keys(value).join(","),
      Object.hasOwn(value, "wasiImport"),
      typeof value.wasiImport,
    );
  } catch (err: any) {
    console.log(label + ": throw", err?.name, err?.code || "no-code");
  }
}

console.log("default export identity:", (wasiDefault as any).WASI === W);
console.log("WASI type/name/length:", typeof W, W.name, W.length);
check("call valid", () => W({ version: "preview1" }));
check("missing", () => new W());
check("empty", () => new W({}));
check("null", () => new W(null));
check("numeric version", () => new W({ version: 1 }));
check("bad version", () => new W({ version: "bad" }));
check("bad args", () => new W({ version: "preview1", args: "cmd" }));
check("bad env", () => new W({ version: "preview1", env: "A=B" }));
check("bad returnOnExit", () => new W({ version: "preview1", returnOnExit: "yes" }));
check("bad stdin", () => new W({ version: "preview1", stdin: "0" }));
check("preview1", () => new W({ version: "preview1" }));
check("unstable", () => new W({ version: "unstable" }));
check("accepted options", () => new W({
  version: "preview1",
  args: ["cmd", "--flag"],
  env: { A: "B" },
  preopens: { "/sandbox": "/tmp" },
  returnOnExit: true,
  stdin: 0,
  stdout: 1,
  stderr: 2,
}));
