const order: string[] = [];

process.on("evt-advanced", () => order.push("on"));
process.prependListener("evt-advanced", () => order.push("prepend"));
process.once("evt-advanced", (a: string, b: string) => order.push(`once:${a}:${b}`));
process.prependOnceListener("evt-advanced", () => order.push("prependOnce"));

console.log("max before:", process.getMaxListeners());
console.log("set chain:", typeof process.setMaxListeners(12).emit === "function");
console.log("max after:", process.getMaxListeners());
console.log("emit first:", process.emit("evt-advanced" as any, "x", "y"));
console.log("emit second:", process.emit("evt-advanced" as any, "x", "y"));
console.log("order:", order.join(","));
