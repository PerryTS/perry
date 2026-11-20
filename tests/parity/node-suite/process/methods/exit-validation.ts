import { spawnSync } from "node:child_process";

const marker = "__perry_exit_validation_child__";
const markerIndex = process.argv.indexOf(marker);

function runCase(label: string): void {
  switch (label) {
    case "none":
      process.exit();
      break;
    case "undefined":
      process.exit(undefined as any);
      break;
    case "null":
      process.exit(null as any);
      break;
    case "number":
      process.exit(3);
      break;
    case "string-int":
      process.exit("2" as any);
      break;
    case "string-hex":
      process.exit("0x10" as any);
      break;
    case "negative":
      process.exit(-1);
      break;
    case "wrap":
      process.exit(256);
      break;
    case "fraction":
      try {
        process.exit(1.9 as any);
        console.log("no-throw");
      } catch (err: any) {
        console.log("THROW", err?.name, err?.code);
      }
      break;
    case "nan":
      try {
        process.exit(NaN as any);
        console.log("no-throw");
      } catch (err: any) {
        console.log("THROW", err?.name, err?.code);
      }
      break;
    case "bad-string":
      try {
        process.exit("abc" as any);
        console.log("no-throw");
      } catch (err: any) {
        console.log("THROW", err?.name, err?.code);
      }
      break;
    case "fraction-string":
      try {
        process.exit("2.5" as any);
        console.log("no-throw");
      } catch (err: any) {
        console.log("THROW", err?.name, err?.code);
      }
      break;
    case "boolean":
      try {
        process.exit(true as any);
        console.log("no-throw");
      } catch (err: any) {
        console.log("THROW", err?.name, err?.code);
      }
      break;
  }
}

if (markerIndex >= 0) {
  runCase(process.argv[markerIndex + 1]);
} else {
  const labels = [
    "none",
    "undefined",
    "null",
    "number",
    "string-int",
    "string-hex",
    "negative",
    "wrap",
    "fraction",
    "nan",
    "bad-string",
    "fraction-string",
    "boolean",
  ];
  const selfArgs = (label: string) => {
    const script = process.argv[1];
    return typeof script === "string" && script.endsWith(".ts")
      ? [script, marker, label]
      : [marker, label];
  };

  for (const label of labels) {
    const result = spawnSync(process.execPath, selfArgs(label), { encoding: "utf8" });
    const firstLine = (result.stdout || "").trim().split("\n")[0] || "no-output";
    console.log(label, "status", result.status, firstLine);
  }
}
