import { request } from "node:http";

for (const path of ["/ok", "", 12, "/space here", "/line\nbreak"] as any[]) {
  try {
    const req = request({
      host: "example.test",
      agent: { addRequest() {} } as any,
      path,
    });
    req.on("error", () => {});
    console.log(JSON.stringify(path), "ok", req.path);
    req.destroy();
  } catch (error: any) {
    console.log(JSON.stringify(path), error.name, error.code);
  }
}
