import { WASI } from "node:wasi";

const W: any = WASI;

try {
  W({ version: "preview1" });
  console.log("call: accepted");
} catch (error: any) {
  console.log("call:", error?.name, error?.code || "no-code");
}
