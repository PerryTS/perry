function logCall(label: string, fn: () => string) {
  try {
    console.log(label, "ok", JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message, err instanceof TypeError);
  }
}

const typed = "aba";
const localNonGlobal: RegExp = /a/;

logCall("typed replaceAll non-global literal", () => typed.replaceAll(/a/, "x"));
logCall("typed replaceAll non-global local", () => typed.replaceAll(localNonGlobal, "x"));
logCall("typed replaceAll global", () => typed.replaceAll(/a/g, "x"));
logCall("typed replaceAll string", () => typed.replaceAll("a", "x"));
logCall("typed replace non-global", () => typed.replace(/a/, "x"));
logCall("typed replaceAll function non-global", () => typed.replaceAll(/a/, () => "x"));
logCall("typed replaceAll function global", () => typed.replaceAll(/a/g, () => "x"));

const dynamic: any = "aba";
logCall("dynamic replaceAll non-global", () => dynamic.replaceAll(/a/, "x"));
logCall("dynamic replaceAll global", () => dynamic.replaceAll(/a/g, "x"));
logCall("dynamic replaceAll string", () => dynamic.replaceAll("a", "x"));
logCall("dynamic replace non-global", () => dynamic.replace(/a/, "x"));
