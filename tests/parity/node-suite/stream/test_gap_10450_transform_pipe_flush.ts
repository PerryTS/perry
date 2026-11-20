import { PassThrough, Transform } from "stream";

function collect(label: string, stream: Transform): void {
  let output = "";
  stream.on("data", (chunk) => {
    output += chunk;
  });
  stream.on("end", () => {
    console.log(label, JSON.stringify(output));
  });
}

const direct = new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, String(chunk).toUpperCase());
  },
  flush(callback) {
    callback(null, "|flushed");
  },
});
collect("direct", direct);
direct.write("ab");
direct.end("cd");

const source = new PassThrough();
const piped = source.pipe(new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, String(chunk).toUpperCase());
  },
  flush(callback) {
    callback(null, "|flushed");
  },
}));
collect("piped", piped);
source.write("ab");
source.end("cd");

const asyncSource = new PassThrough();
const asyncPiped = asyncSource.pipe(new Transform({
  transform(chunk, _encoding, callback) {
    callback(null, String(chunk).toUpperCase());
  },
  flush(callback) {
    queueMicrotask(() => callback(null, "|async-flushed"));
  },
}));
collect("async piped", asyncPiped);
asyncSource.end("ef");
