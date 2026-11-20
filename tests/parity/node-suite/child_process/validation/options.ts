import { ChildProcess, fork, spawn, spawnSync } from "node:child_process";

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

console.log("constructor type:", typeof ChildProcess);
const directChild = new ChildProcess();
console.log("constructor instance:", directChild instanceof ChildProcess);
console.log(
  "constructor initial:",
  directChild.pid === undefined,
  directChild.connected,
  directChild.killed,
  directChild.exitCode,
  directChild.signalCode,
);
for (const [label, options] of [
  ["undefined options", undefined],
  ["null options", null],
  ["string options", "options"],
  ["number options", 1],
] as const) {
  report(`constructor ${label}`, () => directChild.spawn(options as any));
}
for (const [label, file] of [
  ["undefined file", undefined],
  ["null file", null],
  ["number file", 1],
  ["object file", {}],
] as const) {
  report(`constructor ${label}`, () =>
    new ChildProcess().spawn({ file } as any),
  );
}
for (const [label, args] of [
  ["null args", null],
  ["number args", 1],
  ["string args", "args"],
  ["object args", {}],
] as const) {
  report(`constructor ${label}`, () =>
    new ChildProcess().spawn({ file: "node", args } as any),
  );
}
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

for (const value of [0, true, "arg", {}]) {
  report(`fork args ${String(value)}`, () => fork("unused.js", value as any));
}
for (const value of [0, true, "options", []]) {
  report(`fork options ${String(value)}`, () =>
    fork("unused.js", [], value as any),
  );
}

report("spawn stdio string", () =>
  spawn("node", [], { stdio: "other" as any }),
);
report("spawn stdio number", () => spawn("node", [], { stdio: 600 as any }));
report("spawn stdio entry", () =>
  spawn("node", [], { stdio: ["other"] as any }),
);
report("spawn stdio object", () => spawn("node", [], { stdio: [{}] as any }));
report("spawn two ipc", () =>
  spawn("node", [], {
    stdio: ["ignore", "ignore", "ignore", "ipc", "ipc"],
  }),
);
report("spawnSync ipc", () =>
  spawnSync("node", [], {
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  }),
);

const killChild = spawn("node", ["-e", "setInterval(() => {}, 1000)"], {
  stdio: "ignore",
});
function reportKill(label: string, signal: any) {
  try {
    console.log(`${label}:`, killChild.kill(signal));
  } catch (error: any) {
    console.log(`${label}:`, error?.constructor?.name, error?.code);
  }
}
reportKill("kill unknown name", "NOT_A_SIGNAL");
reportKill("kill zero", 0);
reportKill("kill fraction", 3.14);
reportKill("kill object", {});
console.log("kill cleanup:", killChild.kill());
await new Promise((resolve) => killChild.on("close", resolve));
