import { domainToASCII, domainToUnicode } from "node:url";

function probe(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", String(fn()));
  } catch (err: any) {
    console.log(
      label,
      "THROW",
      err?.name || "no-name",
      err?.code || "no-code",
      String(err?.message).split("\n")[0],
    );
  }
}

probe("ascii number", () => domainToASCII(123 as any));
probe("ascii null", () => domainToASCII(null as any));
probe("ascii object", () => domainToASCII({ toString() { return "mañana.com"; } } as any));
probe("ascii symbol", () => domainToASCII(Symbol("x") as any));

probe("unicode number", () => domainToUnicode(123 as any));
probe("unicode null", () => domainToUnicode(null as any));
probe("unicode object", () => domainToUnicode({ toString() { return "xn--maana-pta.com"; } } as any));
probe("unicode symbol", () => domainToUnicode(Symbol("x") as any));
