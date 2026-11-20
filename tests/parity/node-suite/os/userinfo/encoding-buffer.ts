import os from "node:os";

try {
  const info: any = os.userInfo({ encoding: "buffer" as any });
  console.log("username buffer:", Buffer.isBuffer(info.username));
  console.log("homedir buffer:", Buffer.isBuffer(info.homedir));
  console.log("uid type:", typeof info.uid);
} catch (err: any) { console.log("userinfo buffer:", err?.name, err?.code || "no-code"); }
