import { Session } from "node:inspector";

const session = new Session();
session.connect();
const post = (method: string) =>
  new Promise<any>((resolve, reject) =>
    session.post(method, (err, value) => err ? reject(err) : resolve(value))
  );
try {
  for (const domain of ["Debugger", "Profiler", "HeapProfiler"] as const) {
    const enabled = await post(`${domain}.enable`);
    const disabled = await post(`${domain}.disable`);
    console.log(
      domain,
      Object.keys(enabled).sort().join(",") || "<empty>",
      Object.keys(disabled).sort().join(",") || "<empty>",
    );
  }
} finally {
  session.disconnect();
}
