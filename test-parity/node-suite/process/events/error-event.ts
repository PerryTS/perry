process.removeAllListeners("error");

try {
  process.emit("error" as any, new Error("boom"));
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
