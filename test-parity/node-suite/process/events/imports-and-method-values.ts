import processModule from "node:process";

function hasEventMethods(value: any): boolean {
  return (
    typeof value.on === "function" &&
    typeof value.addListener === "function" &&
    typeof value.once === "function" &&
    typeof value.prependListener === "function" &&
    typeof value.prependOnceListener === "function" &&
    typeof value.emit === "function" &&
    typeof value.listeners === "function" &&
    typeof value.rawListeners === "function" &&
    typeof value.eventNames === "function" &&
    typeof value.listenerCount === "function" &&
    typeof value.removeListener === "function" &&
    typeof value.off === "function" &&
    typeof value.removeAllListeners === "function" &&
    typeof value.setMaxListeners === "function" &&
    typeof value.getMaxListeners === "function"
  );
}

const capturedEmit = process.emit;
const capturedListenerCount = process.listenerCount;
const importedEmit = processModule.emit;

console.log("global methods:", hasEventMethods(process));
console.log("default import methods:", hasEventMethods(processModule));
console.log(
  "captured method values:",
  `${typeof capturedEmit}:${typeof capturedListenerCount}:${typeof importedEmit}`,
);

let imported = "";
processModule.on("evt-imported", (a: string, b: string) => {
  imported = `${a}:${b}`;
});
console.log("default import emit:", processModule.emit("evt-imported" as any, "a", "b"));
console.log("default import value:", imported);
processModule.removeAllListeners("evt-imported");
