// A ReadableStream whose pull() throws must ERROR the stream: pending read()s
// reject with the thrown value and the process stays alive (it must NOT crash
// by unwinding out of the internal pull microtask). Byte streams take a
// separate perry pull path, so cover both.
async function probe(label: string, opts: any) {
  const rs = new ReadableStream(opts);
  const reader = rs.getReader();
  try {
    await reader.read();
    console.log(label + ": UNEXPECTED resolve");
  } catch (e) {
    console.log(label + ": rejected: " + (e as Error).message);
  }
}
async function main() {
  await probe("default-pull-throw", { pull() { throw new Error("boom-default"); } });
  await probe("byte-pull-throw", { type: "bytes", pull() { throw new Error("boom-byte"); } });
  console.log("process survived");
}
main();
