import os from "node:os";

const load = os.loadavg();
console.log("load shape:", Array.isArray(load), load.length, load.every(n => typeof n === "number"));
const cpus = os.cpus();
console.log("cpus shape:", Array.isArray(cpus), cpus.length >= 0);
const cpu: any = cpus[0];
if (cpu) console.log("cpu fields:", typeof cpu.model, typeof cpu.speed, typeof cpu.times?.user);
