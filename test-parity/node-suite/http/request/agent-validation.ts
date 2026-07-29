import { request } from "node:http";

for (const agent of [true, {}, { addRequest() {} }] as any[]) {
  try {
    const req = request({ host: "example.test", agent });
    req.on("error", () => {});
    console.log(String(agent), "ok", req.agent === agent);
    req.destroy();
  } catch (error: any) {
    console.log(String(agent), error.name, error.code);
  }
}
