import { IncomingMessage, ServerResponse } from "node:http";

function make() {
  return new ServerResponse(new IncomingMessage(null as any));
}

for (const status of [99, 200, 999, 1000, "201", NaN] as any[]) {
  try {
    const response = make();
    console.log(
      String(status),
      response.writeHead(status) === response,
      response.statusCode,
      response.statusMessage,
    );
  } catch (error: any) {
    console.log(String(status), error.name, error.code);
  }
}

try {
  make().writeHead(200, "bad\nmessage");
} catch (error: any) {
  console.log("message", error.name, error.code);
}
