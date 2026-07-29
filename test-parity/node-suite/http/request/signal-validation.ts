import { request } from "node:http";

for (const signal of [null, {}, "signal"] as any[]) {
  try {
    const req = request({
      host: "example.test",
      agent: { addRequest() {} } as any,
      signal,
    });
    req.on("error", () => {});
    console.log(String(signal), "ok");
    req.destroy();
  } catch (error: any) {
    console.log(String(signal), error.name, error.code);
  }
}
