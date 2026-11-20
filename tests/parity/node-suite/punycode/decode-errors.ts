(process as any).noDeprecation = true;
const punycode = (process as any).getBuiltinModule("punycode");

function show(label: string, fn: () => string) {
  try {
    console.log(label, "ok", fn());
  } catch (error: any) {
    console.log(label, "throw", error.name, error.message);
  }
}

show("decode valid", () => punycode.decode("maana-pta"));
show("decode invalid digit", () => punycode.decode("^"));
show("decode invalid trailing", () => punycode.decode("-"));
show("decode nonbasic", () => punycode.decode("é-"));
show("toUnicode valid", () => punycode.toUnicode("xn--maana-pta.com"));
show("toUnicode invalid", () => punycode.toUnicode("xn---.com"));
show("toUnicode local invalid", () => punycode.toUnicode("xn---@.com"));
show("toUnicode nonbasic", () => punycode.toUnicode("xn--é-.com"));
