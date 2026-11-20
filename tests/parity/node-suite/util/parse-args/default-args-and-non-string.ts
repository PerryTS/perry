// parity-argv: --foo value --bar pos
import { parseArgs } from "node:util";

function tokenSummary(tokens: any[] | undefined): string[] {
  return (tokens || []).map((token: any) => {
    const value = token.value === undefined ? "" : typeof token.value + ":" + String(token.value);
    const inline = token.inlineValue === undefined ? "" : String(token.inlineValue);
    return [token.kind, token.name || "", token.rawName || "", value, inline, token.index].join("|");
  });
}

function show(label: string, value: unknown): void {
  console.log(label + ":", JSON.stringify(value));
}

const fromArgv = parseArgs({
  options: { foo: { type: "string" }, bar: { type: "boolean" } },
  allowPositionals: true,
  tokens: true,
});
show("default args", {
  values: fromArgv.values,
  positionals: fromArgv.positionals,
  tokens: tokenSummary(fromArgv.tokens),
});

const nonStrings = parseArgs({
  args: [1, true, "tail"],
  allowPositionals: true,
  tokens: true,
});
show("non-string positionals", {
  positionals: nonStrings.positionals.map((value: any) => typeof value + ":" + String(value)),
  tokens: tokenSummary(nonStrings.tokens),
});

try {
  parseArgs({ args: ["--name", 3], options: { name: { type: "string" } } });
  console.log("non-string option value: no throw");
} catch (err: any) {
  console.log("non-string option value:", err?.name, err?.code || "no-code");
}
