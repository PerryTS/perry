// node:punycode ucs2 invalid input behavior (#3100).
(process as any).noDeprecation = true;
const punycode = (process as any).getBuiltinModule("punycode");

function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "OK", fn());
  } catch (err) {
    const e = err as { name?: string; code?: string; message?: string };
    console.log(label, "THROW", e.name, e.code ?? "no-code", e.message);
  }
}

show("decode undefined", () => JSON.stringify(punycode.ucs2.decode(undefined as any)));
show("decode null", () => JSON.stringify(punycode.ucs2.decode(null as any)));
show("encode undefined", () => punycode.ucs2.encode(undefined as any));
show("encode null", () => punycode.ucs2.encode(null as any));
show("encode string", () => punycode.ucs2.encode("abc" as any));
show("encode nan", () => punycode.ucs2.encode([NaN]));
show("encode negative", () => punycode.ucs2.encode([-1]));
show("encode fractional", () => punycode.ucs2.encode([3.14]));
show("encode too large", () => punycode.ucs2.encode([0x110000]));
show("encode still valid", () => punycode.ucs2.encode([97, 128512, 98]));
