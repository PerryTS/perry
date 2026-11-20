const { parentPort, workerData } = require("node:worker_threads");

async function main() {
  const fromMap = workerData.map?.get?.("handle");
  const fromSet = workerData.set ? Array.from(workerData.set)[0] : undefined;
  const values = [workerData.direct, workerData.alias, fromMap, fromSet];
  let read;

  try {
    const buffer = Buffer.alloc(5);
    const result = await workerData.direct.read(buffer, 0, buffer.length, 0);
    read = `${result.bytesRead}:${buffer.toString("utf8")}`;
  } catch (error) {
    read = `${error?.name}:${error?.code ?? ""}`;
  }

  parentPort.postMessage({
    same: values.every((value) => value === values[0]),
    brand: workerData.direct?.constructor?.name,
    read,
  });
  await workerData.direct?.close?.().catch?.(() => {});
}

main().catch((error) => {
  parentPort.postMessage({ unexpected: `${error?.name}:${error?.message}` });
});
