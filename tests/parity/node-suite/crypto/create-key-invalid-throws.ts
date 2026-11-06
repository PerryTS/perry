import { createPrivateKey, createPublicKey, createSecretKey } from "node:crypto";
let priv = false;
try { createPrivateKey("test-secret-key"); } catch { priv = true; }
if (!priv) throw new Error("createPrivateKey should throw on a non-PEM string");
let pub = false;
try { createPublicKey("test-secret-key"); } catch { pub = true; }
if (!pub) throw new Error("createPublicKey should throw on a non-PEM string");
const sk: any = createSecretKey(Buffer.from("test-secret-key"));
if (sk.type !== "secret") throw new Error("createSecretKey type: " + sk.type);
console.log("OK");
