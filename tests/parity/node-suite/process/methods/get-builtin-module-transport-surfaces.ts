import process from "node:process";

const samples: Record<string, readonly string[]> = {
  url: ["URL", "URLSearchParams", "format"],
  stream: ["Readable", "Writable", "Stream"],
  https: ["request", "createServer", "Agent"],
  http2: ["connect", "createServer", "constants"],
  child_process: ["spawn", "exec", "ChildProcess"],
  cluster: ["isPrimary", "setupPrimary", "Worker"],
  worker_threads: ["isMainThread", "Worker", "MessageChannel"],
};

for (const [name, keys] of Object.entries(samples)) {
  const mod = process.getBuiltinModule(name) as any;
  console.log("module:", name, !!mod && (typeof mod === "object" || typeof mod === "function"));
  console.log("keys:", name, keys.map((key) => Object.keys(mod ?? {}).includes(key)).join(","));
  console.log("types:", name, keys.map((key) => `${key}:${typeof mod?.[key]}`).join(","));
}
