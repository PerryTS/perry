import { EventEmitter } from "node:events";

const em = new EventEmitter();
const calls: string[] = [];

function numberListener() {
  calls.push("number");
}
function nullListener() {
  calls.push("null");
}
function undefinedListener() {
  calls.push("undefined");
}

em.on(123 as any, numberListener);
em.on(null as any, nullListener);
em.on(undefined as any, undefinedListener);

console.log(
  "names:",
  em.eventNames().map((name) => `${typeof name}:${String(name)}`).join("|"),
);
console.log("emit number:", em.emit(123 as any));
console.log("emit null:", em.emit(null as any));
console.log("emit undefined:", em.emit(undefined as any));
console.log("calls:", calls.join("|"));
console.log("count number string:", em.listenerCount("123"));

em.removeListener(123 as any, numberListener);
console.log("after remove number:", em.listenerCount("123"));

em.removeAllListeners(undefined as any);
console.log(
  "after remove undefined:",
  em.listenerCount("undefined"),
  em.listenerCount("null"),
);

em.removeAllListeners(null as any);
console.log("after remove null:", em.listenerCount("null"));

em.removeAllListeners();
console.log("after remove all:", em.eventNames().length);
