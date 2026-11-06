import { domainToASCII, domainToUnicode } from "node:url";

function show(label: string, fn: () => string) {
  try {
    console.log(label + ":", fn());
  } catch (err: any) {
    console.log(label + " err:", err?.name, err?.code || "no-code", err?.message);
  }
}

show("ascii bool", () => domainToASCII(false as any));
show("unicode null", () => domainToUnicode(null as any));
show("ascii undefined", () => domainToASCII(undefined as any));
show("unicode object", () => domainToUnicode({ toString() { return "xn--bcher-kva.example"; } } as any));
show("ascii symbol", () => domainToASCII(Symbol("x") as any));
show("unicode symbol", () => domainToUnicode(Symbol("x") as any));
