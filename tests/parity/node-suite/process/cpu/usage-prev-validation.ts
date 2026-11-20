function show(label: string, fn: () => { user: number; system: number }) {
  try {
    const value = fn();
    console.log(
      label,
      "OK",
      typeof value.user,
      typeof value.system,
      Number.isInteger(value.user),
      Number.isInteger(value.system),
    );
  } catch (err: any) {
    console.log(label, "THROW", err?.name, err?.code);
  }
}

const invalidPriors: [string, any][] = [
  ["empty-object", {}],
  ["array", []],
  ["string-fields", { user: "1", system: "2" }],
  ["nan-user", { user: NaN, system: 0 }],
  ["infinity-user", { user: Infinity, system: 0 }],
  ["negative-user", { user: -1, system: 0 }],
  ["too-large-user", { user: Number.MAX_SAFE_INTEGER + 1, system: 0 }],
  ["missing-system", { user: 1 }],
  ["missing-user", { system: 1 }],
  ["truthy-string", "x"],
  ["truthy-number", 1],
];

show("cpu null", () => process.cpuUsage(null as any));
show("cpu zero", () => process.cpuUsage(0 as any));
show("cpu max-safe", () => process.cpuUsage({
  user: Number.MAX_SAFE_INTEGER,
  system: 0,
}));
for (const [label, prior] of invalidPriors) {
  show(`cpu ${label}`, () => process.cpuUsage(prior));
}

const first = process.threadCpuUsage();
show("thread previous", () => process.threadCpuUsage(first));
show("thread null", () => process.threadCpuUsage(null as any));
show("thread zero", () => process.threadCpuUsage(0 as any));
show("thread max-safe", () => process.threadCpuUsage({
  user: Number.MAX_SAFE_INTEGER,
  system: 0,
}));
for (const [label, prior] of invalidPriors) {
  show(`thread ${label}`, () => process.threadCpuUsage(prior));
}
