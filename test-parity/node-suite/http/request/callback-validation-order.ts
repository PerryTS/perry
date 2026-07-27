import { request } from "node:http";

function check(label: string, input: any, callback: any) {
  try {
    const req = request(input, { agent: { addRequest() {} } as any }, callback);
    console.log(label, "ok");
    req.destroy();
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

check("valid input invalid callback", "http://example.test/", 1);
check("invalid input valid callback", "not a url", () => {});
check("both invalid", "not a url", 1);
check("null callback", "http://example.test/", null);
