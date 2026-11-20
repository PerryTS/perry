import * as dgram from "node:dgram";

const owner = dgram.createSocket("udp4");
const candidate = dgram.createSocket("udp4");
try {
  await new Promise<void>((resolve) => owner.bind(0, "127.0.0.1", resolve));
  const results: string[] = [];

  for (let attempt = 0; attempt < 2; attempt++) {
    const result = await new Promise<string>((resolve) => {
      candidate.once("error", (value) => {
        const error = value as Error & { code?: string; syscall?: string };
        resolve(`${error.code}:${error.syscall}`);
      });
      candidate.bind(
        owner.address().port,
        "127.0.0.1",
        () => resolve("listening"),
      );
    });
    results.push(result);
    if (result === "listening") break;
  }

  console.log("retry results:", results.join(","));
  let addressState: string;
  try {
    addressState = `port-${candidate.address().port}`;
  } catch (error: unknown) {
    addressState = (error as { code?: string }).code ?? "Error";
  }
  console.log("address after errors:", addressState);
} finally {
  await Promise.all([
    new Promise<void>((resolve) => owner.close(resolve)),
    new Promise<void>((resolve) => candidate.close(resolve)),
  ]);
}
