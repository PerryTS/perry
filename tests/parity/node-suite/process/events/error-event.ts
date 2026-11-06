process.removeAllListeners("error");

try {
  process.emit("error" as any);
  console.log("missing error threw:", false);
} catch (err: any) {
  console.log("missing error threw:", err instanceof Error, err.code, err.message);
}

try {
  process.emit("error" as any, "boom");
  console.log("string error threw:", false);
} catch (err: any) {
  console.log("string error threw:", err instanceof Error, err.code, err.message);
}

try {
  const same = new Error("boom");
  process.emit("error" as any, same);
  console.log("unhandled error threw:", false);
} catch (err: any) {
  console.log("unhandled error threw:", err instanceof Error);
  console.log("unhandled error message:", err.message);
}

process.on("error", (err: Error) => {
  console.log("handled error listener:", err.message);
});
console.log("handled error emit:", process.emit("error" as any, new Error("handled")));
process.removeAllListeners("error");
