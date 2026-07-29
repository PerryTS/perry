import { IncomingMessage, ServerResponse } from "node:http";

const response = new ServerResponse(new IncomingMessage(null as any));
for (
  const [status, headers] of [
    [100, null],
    [102, { "X-Test": "one" }],
    [103, [["X-Test", "one"]]],
    [101, null],
    [99, null],
    [200, null],
  ] as any[]
) {
  try {
    console.log(status, response.writeInformation(status, headers));
  } catch (error: any) {
    console.log(status, error.name, error.code);
  }
}

for (
  const hints of [{ link: "</style.css>; rel=preload" }, {}, null] as any[]
) {
  try {
    console.log("early", String(response.writeEarlyHints(hints)));
  } catch (error: any) {
    console.log("early", error.name, error.code);
  }
}
