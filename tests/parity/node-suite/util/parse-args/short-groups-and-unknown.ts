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

let parsed = parseArgs({
  args: ["-abc"],
  options: {
    alpha: { type: "boolean", short: "a" },
    beta: { type: "boolean", short: "b" },
    charlie: { type: "boolean", short: "c" },
  },
  tokens: true,
});
show("short bool values", parsed.values);
show("short bool tokens", tokenSummary(parsed.tokens));

parsed = parseArgs({
  args: ["-abcv"],
  options: {
    alpha: { type: "boolean", short: "a" },
    beta: { type: "boolean", short: "b" },
    charlie: { type: "string", short: "c" },
  },
  tokens: true,
});
show("short inline values", parsed.values);
show("short inline tokens", tokenSummary(parsed.tokens));

parsed = parseArgs({
  args: ["-abc", "value"],
  options: {
    alpha: { type: "boolean", short: "a" },
    beta: { type: "boolean", short: "b" },
    charlie: { type: "string", short: "c" },
  },
  tokens: true,
});
show("short next values", parsed.values);
show("short next tokens", tokenSummary(parsed.tokens));

for (const args of [["--unknown", "x", "pos"], ["--unknown=x", "pos"], ["-x", "pos"], ["-abc", "pos"]]) {
  parsed = parseArgs({
    args,
    strict: false,
    allowPositionals: true,
    options: { known: { type: "boolean", short: "k" } },
    tokens: true,
  });
  show("unknown " + args.join(" "), {
    values: parsed.values,
    positionals: parsed.positionals,
    tokens: tokenSummary(parsed.tokens),
  });
}
