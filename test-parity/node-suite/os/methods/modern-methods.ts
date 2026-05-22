import os from "node:os";

try {
  const value = os.availableParallelism();
  console.log("availableParallelism:", typeof value, String(value).length > 0);
} catch (err: any) { console.log("availableParallelism:", err?.name, err?.code || "no-code"); }

try {
  const value = os.machine();
  console.log("machine:", typeof value, String(value).length > 0);
} catch (err: any) { console.log("machine:", err?.name, err?.code || "no-code"); }

try {
  const value = os.version();
  console.log("version:", typeof value, String(value).length > 0);
} catch (err: any) { console.log("version:", err?.name, err?.code || "no-code"); }

try {
  const value = os.endianness();
  console.log("endianness:", typeof value, String(value).length > 0);
} catch (err: any) { console.log("endianness:", err?.name, err?.code || "no-code"); }
