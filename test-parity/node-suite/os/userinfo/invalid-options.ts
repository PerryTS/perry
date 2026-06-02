import os from "node:os";

for (const options of [null, { encoding: "madeup" }, { encoding: 123 }] as any[]) {
  try { console.log("userinfo:", JSON.stringify(options), typeof os.userInfo(options).username); } catch (err: any) { console.log("userinfo:", JSON.stringify(options), err?.name, err?.code || "no-code"); }
}
