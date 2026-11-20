// #9616: Readable.toWeb() must consume foreign/event-backed Readables such as
// fs.ReadStream. The old adapter only inspected node:stream's private chunk
// buffer, mistook an fs stream for an already-drained stream, and closed the
// Web stream with zero bytes on its first pull.
import {
  createReadStream,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Readable } from "node:stream";

function digest(bytes: Uint8Array): string {
  let hash = 2166136261;
  for (let i = 0; i < bytes.length; i++) {
    hash = Math.imul(hash ^ bytes[i], 16777619) >>> 0;
  }
  return hash.toString(16);
}

async function consume(stream: ReadableStream<any>): Promise<{
  bytes: number;
  chunks: number;
  digest: string;
}> {
  const reader = stream.getReader();
  let bytes = 0;
  let chunks = 0;
  let hash = 2166136261;
  for (;;) {
    const result = await reader.read();
    if (result.done) break;
    chunks++;
    const value = result.value as Uint8Array;
    bytes += value.length;
    for (let i = 0; i < value.length; i++) {
      hash = Math.imul(hash ^ value[i], 16777619) >>> 0;
    }
  }
  return { bytes, chunks, digest: hash.toString(16) };
}

const directory = mkdtempSync(join(tmpdir(), "perry-readable-to-web-"));
const path = join(directory, "payload.bin");
const emptyPath = join(directory, "empty.bin");
const missingPath = join(directory, "missing.bin");

try {
  const payload = new Uint8Array(150_321);
  for (let i = 0; i < payload.length; i++) payload[i] = (i * 31 + 7) & 255;
  writeFileSync(path, payload);
  writeFileSync(emptyPath, new Uint8Array(0));
  const expectedDigest = digest(payload);

  const nodeSource = createReadStream(path, { highWaterMark: 4096 });
  const viaReader = await consume(Readable.toWeb(nodeSource) as ReadableStream<any>);
  console.log(
    "reader:",
    viaReader.bytes,
    viaReader.digest === expectedDigest,
    viaReader.chunks > 1,
    nodeSource.bytesRead,
  );

  const responseBytes = new Uint8Array(
    await new Response(
      Readable.toWeb(
        createReadStream(path, { highWaterMark: 8192 }),
      ) as ReadableStream<any>,
    ).arrayBuffer(),
  );
  console.log(
    "response:",
    responseBytes.length,
    digest(responseBytes) === expectedDigest,
  );

  // Exercise the runtime method-value fallback as well as the compiler's
  // direct static-method lowering above.
  const dynamicMethodName = ["to", "Web"].join("");
  const dynamicToWeb = (Readable as any)[dynamicMethodName];
  const dynamic = await consume(
    dynamicToWeb(createReadStream(path, { highWaterMark: 5000 })),
  );
  console.log(
    "dynamic:",
    dynamic.bytes,
    dynamic.digest === expectedDigest,
    dynamic.chunks > 1,
  );

  const rangeStart = 123;
  const rangeEnd = 8122;
  const range = await consume(
    Readable.toWeb(
      createReadStream(path, {
        start: rangeStart,
        end: rangeEnd,
        highWaterMark: 777,
      }),
    ) as ReadableStream<any>,
  );
  const expectedRange = payload.slice(rangeStart, rangeEnd + 1);
  console.log(
    "range:",
    range.bytes,
    range.digest === digest(expectedRange),
    range.chunks > 1,
  );

  const empty = await (
    Readable.toWeb(createReadStream(emptyPath)) as ReadableStream<any>
  ).getReader().read();
  console.log("empty:", empty.done, empty.value === undefined);

  let rejected = false;
  try {
    await (
      Readable.toWeb(createReadStream(missingPath)) as ReadableStream<any>
    ).getReader().read();
  } catch {
    rejected = true;
  }
  console.log("missing rejects:", rejected);

  const canceledSource = createReadStream(path, { highWaterMark: 1024 });
  const canceledReader = (
    Readable.toWeb(canceledSource) as ReadableStream<any>
  ).getReader();
  const first = await canceledReader.read();
  const stoppedBeforeWholeFile = canceledSource.bytesRead < payload.length;
  await canceledReader.cancel("stop");
  await new Promise<void>((resolve) => canceledSource.once("close", resolve));
  console.log(
    "cancel:",
    !first.done && first.value.length === 1024,
    stoppedBeforeWholeFile,
    canceledSource.destroyed,
    canceledSource.closed,
  );

  // Keep coverage for the existing private-buffer path: foreign-stream support
  // must not regress Readable.from()/custom Readables.
  const classic = await consume(
    Readable.toWeb(Readable.from(["alpha", "beta"])) as ReadableStream<any>,
  );
  console.log("classic:", classic.bytes, classic.chunks);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
