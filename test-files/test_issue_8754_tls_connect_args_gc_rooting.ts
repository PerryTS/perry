// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1
// `tls.connect` consults a user-replaced createSecureContext before it resolves
// either overload. The callback forces a moving collection while the original
// options object and secureConnect callback exist only in the external-net FFI
// arguments. Both must be re-read from transient roots after the callback.
import tls from "node:tls";
import { readFileSync } from "node:fs";
import { isIP } from "node:net";

declare function gc(): void;

const fixture = new URL("../test-parity/node-suite/tls/fixtures/", import.meta.url);
const key = readFileSync(new URL("localhost-key.pem", fixture));
const cert = readFileSync(new URL("localhost-cert.pem", fixture));
const originalCreateSecureContext = tls.createSecureContext;

if (isIP("127.0.0.1") !== 4) throw new Error("node:net route unavailable");

(tls as any).createSecureContext = (options: any) => {
  const churn: Array<{ value: string }> = [];
  for (let i = 0; i < 2000; i++) churn.push({ value: "tls-root-" + i });
  if (typeof gc === "function") gc();
  return originalCreateSecureContext(options);
};

const server = tls.createServer({ key, cert }, (socket) => socket.end());
server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  const options = { port, host: "127.0.0.1", rejectUnauthorized: false };
  const optionsClient = tls.connect(options, function () {
    console.log(
      "options:",
      this === optionsClient,
      options.host,
      options.rejectUnauthorized,
    );
  });
  optionsClient.on("close", () => {
    const positionalOptions = { rejectUnauthorized: false };
    const positionalClient = tls.connect(
      port,
      "127.0.0.1",
      positionalOptions,
      function () {
        console.log(
          "positional:",
          this === positionalClient,
          positionalOptions.rejectUnauthorized,
        );
      },
    );
    positionalClient.on("close", () => {
      (tls as any).createSecureContext = originalCreateSecureContext;
      server.close();
    });
  });
});
