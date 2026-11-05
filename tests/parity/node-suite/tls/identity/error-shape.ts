import tls from "node:tls";

const cert: any = { subjectaltname: "DNS:good.example", subject: { CN: "fallback.example" } };
const err: any = tls.checkServerIdentity("bad.example", cert);
console.log("class:", err instanceof Error, err.name);
console.log("code:", err.code);
console.log("fields:", err.host, err.cert === cert, typeof err.reason);
console.log("enumerable:", Object.keys(err).sort().join(","));
console.log("message semantic:", err.message.includes("bad.example"), err.message.includes("good.example"));
