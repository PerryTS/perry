function probe(label: string, fn: () => unknown): void {
  try {
    console.log(label, "OK", fn());
  } catch (err: any) {
    console.log(label, "THROW", err.name, err.code, err.message.split("\n")[0]);
  }
}

const first = process.threadCpuUsage();
for (let i = 0; i < 100000; i++) {
  // Burn a tiny amount of CPU so the delta path is exercised without relying
  // on a specific microsecond count.
}
const delta = process.threadCpuUsage(first);
console.log("thread delta shape:", typeof delta.user, typeof delta.system, delta.user >= 0, delta.system >= 0);
console.log("thread nullish:", typeof process.threadCpuUsage(undefined).user, typeof process.threadCpuUsage(null).system);

probe("thread number", () => process.threadCpuUsage(123 as any));
probe("thread array", () => process.threadCpuUsage([] as any));
probe("thread missing field", () => process.threadCpuUsage({ user: 1 } as any));
probe("thread negative field", () => process.threadCpuUsage({ user: -1, system: 0 } as any));
