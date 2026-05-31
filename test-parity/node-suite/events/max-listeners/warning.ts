import { EventEmitter } from "node:events";

const em = new EventEmitter();
const warnings: any[] = [];
const originalEmitWarning = process.emitWarning;

process.emitWarning = function (warning: any) {
  warnings.push({
    name: warning.name,
    message: warning.message,
    type: warning.type,
    count: warning.count,
    emitter: warning.emitter === em,
    thisIsProcess: this === process,
  });
} as any;

function a() {}
function b() {}
function c() {}

em.setMaxListeners(1);
em.on("x", a);
em.on("x", b);
em.on("x", c);
console.log("first warnings:", warnings.length);

em.removeAllListeners("x");
em.on("x", a);
em.on("x", b);
console.log("second warnings:", warnings.length);

for (const warning of warnings) {
  console.log(
    "warning:",
    warning.name,
    warning.type,
    warning.count,
    warning.emitter,
    warning.thisIsProcess,
  );
  console.log("message:", warning.message);
}

process.emitWarning = originalEmitWarning;
