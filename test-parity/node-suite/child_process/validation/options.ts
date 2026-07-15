import { fork, spawn, spawnSync } from "node:child_process";

function report(label: string, action: () => unknown) {
  try {
    const child: any = action();
    if (child && typeof child === "object") {
      child.on?.("error", () => {});
      if (child.connected) child.disconnect();
      child.kill?.();
    }
    console.log(`${label}: no throw`);
  } catch (error: any) {
    console.log(`${label}:`, error?.constructor?.name, error?.code);
  }
}

report("spawn options number", () => spawn("node", [], 1 as any));
report("spawn cwd number", () => spawn("node", [], { cwd: 1 as any }));
report("spawn timeout negative", () => spawn("node", [], { timeout: -1 }));
report("spawn killSignal unknown", () =>
  spawn("node", [], { killSignal: "NOT_A_SIGNAL" }),
);
report("spawn serialization invalid", () =>
  spawn("node", [], { serialization: "other" as any }),
);
report("spawn detached number", () =>
  spawn("node", [], { detached: 1 as any }),
);
report("spawn shell number", () => spawn("node", [], { shell: 1 as any }));
report("spawn argv0 number", () => spawn("node", [], { argv0: 1 as any }));
report("spawnSync timeout string", () =>
  spawnSync("node", [], { timeout: "1" as any }),
);
report("spawnSync maxBuffer negative", () =>
  spawnSync("node", [], { maxBuffer: -1 }),
);
report("spawnSync detached number", () =>
  spawnSync("missing-perry-command", [], { detached: 1 as any }),
);
report("spawnSync shell number", () =>
  spawnSync("missing-perry-command", [], { shell: 1 as any }),
);
report("spawnSync argv0 number", () =>
  spawnSync("missing-perry-command", [], { argv0: 1 as any }),
);
report("fork serialization invalid", () =>
  fork("unused.js", [], { serialization: "other" as any }),
);
