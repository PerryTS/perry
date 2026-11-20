import { parseArgs } from "node:util";

function probe(label: string, config?: any): void {
  try {
    const result = arguments.length === 1 ? parseArgs() : parseArgs(config);
    console.log(label, "OK", JSON.stringify(result.values), result.positionals.join(","));
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code || "no-code", err?.message);
  }
}

probe("omitted");
probe("primitive string", "x");
probe("primitive number", 0);
probe("args number", { args: 0 });
probe("args object", { args: {} });
probe("options array", { options: [] });
probe("options string", { options: "x" });
probe("options null", { options: null });
