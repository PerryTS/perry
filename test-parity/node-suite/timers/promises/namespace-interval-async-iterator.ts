import * as timersPromises from "node:timers/promises";

const ac = new AbortController();
const values: string[] = [];
try {
  for await (const value of timersPromises.setInterval(1, "ns-tick", { signal: ac.signal })) {
    values.push(String(value));
    if (values.length === 2) ac.abort();
  }
} catch (err: any) {
  console.log("namespace interval abort:", err instanceof Error, err?.name, err?.code || "no-code");
}
console.log("namespace values:", values.join(","));
