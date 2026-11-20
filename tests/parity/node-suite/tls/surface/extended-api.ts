import tls from "node:tls";

console.log("alpn converter:", typeof tls.convertALPNProtocols === "function");
console.log(
  "set key cert:",
  typeof tls.TLSSocket.prototype.setKeyCert === "function",
);
console.log(
  "shared sigalgs:",
  typeof tls.TLSSocket.prototype.getSharedSigalgs === "function",
);
console.log(
  "x509 getters:",
  typeof tls.TLSSocket.prototype.getX509Certificate === "function",
  typeof tls.TLSSocket.prototype.getPeerX509Certificate === "function",
);
console.log(
  "legacy pair removed:",
  !("SecurePair" in tls),
  !("createSecurePair" in tls),
);
