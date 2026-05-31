import { EventEmitter } from "node:events";

const seen: string[] = [];
const originalEmitWarning = process.emitWarning;
(process as any).emitWarning = (warning: any) => {
  seen.push([
    warning?.name,
    warning?.type,
    warning?.count,
    warning?.emitter instanceof EventEmitter,
    String(warning?.message).includes("3 z listeners"),
  ].join(":"));
};

const em = new EventEmitter();
em.setMaxListeners(2);
em.on("z", () => {});
em.on("z", () => {});
em.on("z", () => {});
em.on("z", () => {});

const zero = new EventEmitter();
zero.setMaxListeners(0);
zero.on("x", () => {});
zero.on("x", () => {});

const infinite = new EventEmitter();
infinite.setMaxListeners(Infinity);
infinite.on("x", () => {});
infinite.on("x", () => {});

process.emitWarning = originalEmitWarning;
console.log("warnings:", seen.join("|"));
