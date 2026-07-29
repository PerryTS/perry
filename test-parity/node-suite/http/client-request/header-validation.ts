import { request } from "node:http";

const req = request({
  host: "example.test",
  agent: { addRequest() {} } as any,
});
req.on("error", () => {});

function check(label: string, name: any, value: any) {
  try {
    req.setHeader(name, value);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

check("empty name", "", "x");
check("space name", "x bad", "x");
check("undefined value", "x-test", undefined);
check("newline value", "x-test", "a\nb");
check("array value", "x-test", ["a", "b"]);
req.destroy();
