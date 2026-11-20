import { request } from "node:http";

for (const protocol of ["https:", "ftp:", "HTTP:"]) {
  try {
    request({
      protocol,
      host: "example.test",
      agent: { addRequest() {} } as any,
    });
    console.log(protocol, "ok");
  } catch (error: any) {
    console.log(protocol, error.name, error.code);
  }
}
