const events: string[] = [];

process.on("warning", (warning: any) => {
  events.push(`${warning.name}:${warning.code ?? ""}:${warning.message}`);
  console.log("event:", events.join("|"));
});

const ret = process.emitWarning("plain", "CustomWarning", "PERRY_POS");
console.log("return:", String(ret), events.join("|"));

process.nextTick(() => events.push("nextTick"));
setImmediate(() => console.log("final:", events.join("|")));
