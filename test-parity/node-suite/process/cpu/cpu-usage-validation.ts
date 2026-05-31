function probe(label: string, fn: () => unknown): void {
  try {
    console.log(label, "OK", fn());
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

const baseline = process.cpuUsage();
const delta = process.cpuUsage(baseline);
console.log("cpu delta shape:", typeof delta.user, typeof delta.system, delta.user >= 0, delta.system >= 0);
console.log("cpu nullish:", typeof process.cpuUsage(undefined).user, typeof process.cpuUsage(null).system);

probe("cpu empty object", () => process.cpuUsage({} as any));
probe("cpu array", () => process.cpuUsage([] as any));
probe("cpu string fields", () => process.cpuUsage({ user: "1", system: "2" } as any));
probe("cpu nan field", () => process.cpuUsage({ user: NaN, system: Infinity } as any));
probe("cpu negative field", () => process.cpuUsage({ user: -1, system: 0 } as any));
