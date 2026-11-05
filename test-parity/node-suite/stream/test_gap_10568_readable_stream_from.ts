import * as streamWeb from "node:stream/web";

const stream: any = (streamWeb.ReadableStream as any).from(["x", "y", "z"]);
const reader = stream.getReader();

for (let index = 0; index < 4; index++) {
  const result: any = await reader.read();
  console.log(index, result.done, result.value, JSON.stringify(Object.keys(result)));
}
