import * as https from "node:https";

const agent = new https.Agent();
console.log(agent.getName({
  host: "example.test",
  port: 443,
  ca: "ca",
  cert: "cert",
  ciphers: "cipher",
  key: "key",
  maxVersion: "TLSv1.3",
  minVersion: "TLSv1.2",
  rejectUnauthorized: false,
  secureOptions: 0,
  servername: "sni.test",
  sigalgs: ["rsa_pss_rsae_sha256"],
}));
agent.destroy();
