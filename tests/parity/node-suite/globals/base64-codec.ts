import { atob as bufferAtob, btoa as bufferBtoa } from "node:buffer";

function show(label: string, value: unknown) {
  console.log(`${label}:`, value);
}

function showError(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}:`, "no throw");
  } catch (err: any) {
    console.log(`${label}:`, err?.name, err?.code, typeof err?.code);
  }
}

show("global btoa string", btoa("hello"));
show("global btoa number", btoa(123 as any));
show("global btoa latin1", btoa("\u00ff"));
show("global atob whitespace", atob(" aG Vs bG8=\n"));
show("global atob unpadded", atob("YQ"));
show("global atob object", atob({ toString() { return "YQ=="; } } as any));

const encode = globalThis.btoa;
const decode = globalThis.atob;
show("rebound btoa", encode("x"));
show("rebound atob", decode("eA=="));

show("buffer btoa number", bufferBtoa(123 as any));
show("buffer atob object", bufferAtob({ toString() { return "Yg=="; } } as any));

showError("global btoa unicode", () => btoa("✓"));
showError("global atob invalid", () => atob("%%%"));
showError("global atob bad padding", () => atob("YQ="));
showError("buffer btoa unicode", () => bufferBtoa("✓"));
showError("buffer atob invalid", () => bufferAtob("%%%"));
