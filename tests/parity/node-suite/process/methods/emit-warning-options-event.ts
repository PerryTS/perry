const events: string[] = [];

process.on("warning", (warning: any) => {
  events.push(`${warning.name}:${warning.code ?? ""}:${warning.message}:${warning.detail ?? ""}`);
  console.log("event:", events.join("|"));
});

console.log("before");
const ret = process.emitWarning("hello", {
  type: "CustomWarning",
  code: "PERRY_TEST",
  detail: "extra detail",
});
console.log("after:", String(ret), events.join("|"));

process.nextTick(() => events.push("nextTick"));
setImmediate(() => console.log("final:", events.join("|")));
