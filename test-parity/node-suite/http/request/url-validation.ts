import { request } from "node:http";

for (const input of ["not a url", "http://[::1", 12, null] as any[]) {
  try {
    request(input, { agent: { addRequest() {} } as any });
    console.log(String(input), "ok");
  } catch (error: any) {
    console.log(String(input), error.name, error.code);
  }
}

try {
  request(new URL("https://example.test/"), {
    agent: { addRequest() {} } as any,
  });
} catch (error: any) {
  console.log("https URL", error.name, error.code);
}
