import tls from "node:tls";

class Stop extends Error {}
let seen: unknown;
const original = tls.createSecureContext;
(tls as any).createSecureContext = (options: any) => {
  seen = options.ciphers;
  throw new Stop();
};
try {
  tls.connect();
  console.log("stopped: false");
} catch (err) {
  console.log("stopped:", err instanceof Stop);
} finally {
  (tls as any).createSecureContext = original;
}
console.log("default forwarded:", seen === tls.DEFAULT_CIPHERS);
