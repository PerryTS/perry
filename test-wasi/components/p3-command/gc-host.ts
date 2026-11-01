import { readFileSync } from "node:fs";
import { collect } from "perry/gc";
import { runComponentCommand } from "perry/component";

if (process.argv[2] === "native-control") {
  const retained = { text: "native control" };
  collect();
  console.log(`PASS ${retained.text}`);
} else {
  const moving = process.argv[3] === "moving";
  const environment = moving ? {
    PERRY_GC_INSTRUMENTS: "1",
    PERRY_GC_FORCE_EVACUATE: "1",
    PERRY_GC_VERIFY_EVACUATION: "1",
    PERRY_GC_DIAG: "1",
    PERRY_GC_SCHEDULE_SEED: "303",
    PERRY_GC_SCHEDULE_RATE: "127",
  } : {};
  const status = runComponentCommand(readFileSync(process.argv[2]), {
    fuel: 3000000000,
    wasi: { env: environment, inheritStdout: true, inheritStderr: true },
  });
  process.exit(status);
}
