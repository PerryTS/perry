// Gap test: process.stdout / process.stderr expose their Writable buffer state.
// With nothing queued Node reports `writableLength` 0, a positive
// `writableHighWaterMark` and `writableNeedDrain` false, whether the fd is a
// TTY, a pipe or a file. Perry left all three undefined, so the common
// "flush stdio, then process.exit()" helper below never saw an empty buffer
// and always waited out its cap for a 'drain' that never fires (#11418).
//
// The concrete high-water mark is not printed: it is Node's configurable
// default (`stream.getDefaultHighWaterMark()`), not a property of the fd.
const streams: Array<[string, NodeJS.WriteStream]> = [
  ["stdout", process.stdout],
  ["stderr", process.stderr],
];
for (const [name, s] of streams) {
  console.log(name + ".writableLength:", typeof s.writableLength, s.writableLength === 0);
  console.log(
    name + ".writableHighWaterMark:",
    typeof s.writableHighWaterMark,
    s.writableHighWaterMark > 0,
  );
  console.log(name + ".writableNeedDrain:", typeof s.writableNeedDrain, s.writableNeedDrain === false);
}

function drain(stream: NodeJS.WriteStream): Promise<void> {
  if (stream.writableLength === 0) return Promise.resolve();
  return new Promise((resolve) => stream.once("drain", () => resolve()));
}

let cap: ReturnType<typeof setTimeout> | undefined;
const outcome = await Promise.race([
  Promise.all([drain(process.stdout), drain(process.stderr)]).then(() => "drained"),
  new Promise<string>((resolve) => {
    cap = setTimeout(() => resolve("waited out the cap"), 500);
  }),
]);
clearTimeout(cap);
console.log("flush before exit:", outcome);
