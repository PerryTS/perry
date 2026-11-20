import { Worker } from "node:worker_threads";

const source = ["const broken = ", ")"].join("");
let summary = "missing";
let worker: Worker | undefined;

try {
  worker = new Worker(source, { eval: true });
  worker.on("message", () => console.log("unexpected message"));
  worker.on("error", (error: any) => {
    summary = `${error?.constructor === SyntaxError}:${error?.name}`;
  });
  worker.on("exit", (code) => {
    console.log("error:", summary);
    console.log("exit:", code);
  });
} catch (error: any) {
  console.log("construct:", error?.name, error?.code ?? "");
}
