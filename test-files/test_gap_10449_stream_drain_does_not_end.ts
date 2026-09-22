import { PassThrough } from "stream";

const stream = new PassThrough();
stream.on("data", (chunk) => console.log("data", JSON.stringify(String(chunk))));
stream.on("end", () => console.log("end"));
stream.on("error", (error: any) => console.log("error", error.code));

stream.write("a\n");
setTimeout(() => {
  console.log("before second write", stream.writableEnded, stream.readableEnded);
  stream.write("b\n");
  stream.end("c\n");
}, 10);
