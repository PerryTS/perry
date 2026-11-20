import { request } from "node:http";

for (const method of ["get", "M-SEARCH", "", "BAD METHOD", 12] as any[]) {
  try {
    const req = request({
      host: "example.test",
      agent: { addRequest() {} } as any,
      method,
    });
    req.on("error", () => {});
    console.log(JSON.stringify(method), "ok", req.method);
    req.destroy();
  } catch (error: any) {
    console.log(JSON.stringify(method), error.name, error.code);
  }
}
