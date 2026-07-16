import tls from "node:tls";

class Stop extends Error {}
const originalCiphers = tls.DEFAULT_CIPHERS;
const originalCreateContext = tls.createSecureContext;
let seen = "none";
try {
  (tls as any).DEFAULT_CIPHERS = "DEFAULT";
  (tls as any).createSecureContext = (options: any) => {
    seen = options.ciphers;
    throw new Stop();
  };
  try {
    tls.connect();
  } catch (err) {
    console.log("stopped:", err instanceof Stop);
  }
  console.log("mutated:", tls.DEFAULT_CIPHERS, seen);
} finally {
  (tls as any).DEFAULT_CIPHERS = originalCiphers;
  (tls as any).createSecureContext = originalCreateContext;
}
console.log("restored:", tls.DEFAULT_CIPHERS === originalCiphers);
