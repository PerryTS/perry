import { Session } from "node:inspector/promises";

const session = new Session();
session.connect();
try {
  for (const domain of ["Runtime", "Debugger", "Profiler", "HeapProfiler"]) {
    const enabled = await session.post(`${domain}.enable`);
    const disabled = await session.post(`${domain}.disable`);
    console.log(
      domain,
      Reflect.ownKeys(enabled).length,
      Reflect.ownKeys(disabled).length,
    );
  }
} finally {
  session.disconnect();
}
