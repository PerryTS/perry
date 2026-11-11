import process from "node:process";

const bufferMod = process.getBuiltinModule("buffer") as any;
const timersMod = process.getBuiltinModule("timers") as any;
const timersPromisesMod = process.getBuiltinModule("timers/promises") as any;

console.log("buffer object:", bufferMod !== undefined && typeof bufferMod === "object");
console.log("buffer Buffer:", typeof bufferMod?.Buffer);
console.log("buffer isAscii:", typeof bufferMod?.isAscii);
console.log("buffer isUtf8:", typeof bufferMod?.isUtf8);
console.log("buffer ascii result:", bufferMod?.isAscii?.(bufferMod.Buffer.from("hi")));
console.log("buffer keys include isAscii:", Object.keys(bufferMod ?? {}).includes("isAscii"));
console.log("buffer keys include isUtf8:", Object.keys(bufferMod ?? {}).includes("isUtf8"));

console.log("timers object:", timersMod !== undefined && typeof timersMod === "object");
console.log("timers setTimeout:", typeof timersMod?.setTimeout);
console.log("timers promises:", typeof timersMod?.promises);
console.log("timers promises setTimeout:", typeof timersMod?.promises?.setTimeout);
console.log("timers promises scheduler:", typeof timersMod?.promises?.scheduler);
console.log("timers promises scheduler.wait:", typeof timersMod?.promises?.scheduler?.wait);
console.log("timers promises scheduler.yield:", typeof timersMod?.promises?.scheduler?.yield);
console.log("timers keys include promises:", Object.keys(timersMod ?? {}).includes("promises"));

console.log("direct timers promises object:", timersPromisesMod !== undefined && typeof timersPromisesMod === "object");
console.log("direct timers promises setTimeout:", typeof timersPromisesMod?.setTimeout);
console.log("direct timers promises scheduler:", typeof timersPromisesMod?.scheduler);
console.log("direct timers promises scheduler.wait:", typeof timersPromisesMod?.scheduler?.wait);
