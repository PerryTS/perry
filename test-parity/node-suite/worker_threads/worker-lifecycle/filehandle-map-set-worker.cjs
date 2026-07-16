const { parentPort, workerData } = require("node:worker_threads");

async function main() {
  const mapHandle = workerData?.map?.get?.("handle");
  const setHandle = workerData?.set ? Array.from(workerData.set)[0] : undefined;
  const result = {
    mapBrand: workerData?.map?.constructor?.name,
    setBrand: workerData?.set?.constructor?.name,
    handleBrand: mapHandle?.constructor?.name,
    same: mapHandle === setHandle,
    read: "unsupported",
  };
  if (typeof mapHandle?.read === "function") {
    const buffer = Buffer.alloc(1);
    const read = await mapHandle.read(buffer, 0, 1, 0);
    result.read = `${read.bytesRead}:${buffer[0]}`;
    await mapHandle.close();
  }
  parentPort.postMessage(result);
}

main().catch((error) => {
  parentPort.postMessage({ error: `${error?.name}:${error?.code || ""}` });
});
