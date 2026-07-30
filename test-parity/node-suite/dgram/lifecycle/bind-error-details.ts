// Upstream: Node v26.5.0 lib/dgram.js ExceptionWithHostPort bind path.
// Coverage added: public address and port fields on bind conflicts.
import * as dgram from "node:dgram";

const owner = dgram.createSocket("udp4");
const candidate = dgram.createSocket("udp4");
try {
  await new Promise<void>((resolve) => owner.bind(0, "127.0.0.1", resolve));
  const expectedPort = owner.address().port;
  const details = await new Promise<string>((resolve) => {
    candidate.once("error", (error) => {
      resolve(
        `${error.code}:${error.syscall}:${error.address}:${
          error.port === expectedPort
        }`,
      );
    });
    candidate.bind(expectedPort, "127.0.0.1");
  });
  console.log("bind error details:", details);
} finally {
  await Promise.all([
    new Promise<void>((resolve) => candidate.close(resolve)),
    new Promise<void>((resolve) => owner.close(resolve)),
  ]);
}
