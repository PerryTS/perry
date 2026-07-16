import { WASI } from "node:wasi";

const W: any = WASI;

function check(label: string, options: any) {
  try {
    new W({ version: "preview1", ...options });
    console.log(label + ": ok");
  } catch (error: any) {
    console.log(label + ": throw", error?.name, error?.code || "no-code");
  }
}

check("args undefined", { args: undefined });
check("args array", { args: ["tool", 2, true] });
check("args string", { args: "tool" });
check("args object", { args: { 0: "tool", length: 1 } });
check("args null", { args: null });
check("env undefined", { env: undefined });
check("env object", { env: { A: "one", B: 2, OMIT: undefined } });
check("env array", { env: [] });
check("env null", { env: null });
check("env string", { env: "A=one" });
