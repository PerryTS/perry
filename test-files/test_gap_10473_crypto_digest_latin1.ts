import crypto from "node:crypto";

function report(label: string, value: string, expectedHex: string) {
  const codes = Array.from(value.slice(0, 6), (char) => char.charCodeAt(0)).join(",");
  const roundTrips = Buffer.from(value, "latin1").toString("hex") === expectedHex;
  console.log(label, value.length, codes, roundTrips);
}

const hashHex = crypto.createHash("sha256").update("abc").digest("hex");
report("hash latin1", crypto.createHash("sha256").update("abc").digest("latin1"), hashHex);
report("hash binary", crypto.createHash("sha256").update("abc").digest("binary"), hashHex);

const hmacHex = crypto.createHmac("sha256", "k").update("abc").digest("hex");
report("hmac latin1", crypto.createHmac("sha256", "k").update("abc").digest("latin1"), hmacHex);
report("hmac binary", crypto.createHmac("sha256", "k").update("abc").digest("binary"), hmacHex);
