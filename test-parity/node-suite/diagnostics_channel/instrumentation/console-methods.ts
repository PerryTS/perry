import { channel } from "node:diagnostics_channel";

const methods = ["log", "info", "debug", "warn", "error"] as const;
const seen: string[] = [];
const subscribers: Array<[ReturnType<typeof channel>, (args: any[]) => void]> = [];
for (const method of methods) {
  const ch = channel(`console.${method}`);
  const subscriber = (args: any[]) => {
    if (args[0] === "seen:" || args[0] === "mutated:") return;
    seen.push(`${method}:${Array.isArray(args)}:${args[0]}:${args[1]?.key}`);
    if (args[1] && typeof args[1] === "object") args[1].mutated = true;
  };
  ch.subscribe(subscriber);
  subscribers.push([ch, subscriber]);
}
const object: any = { key: "value" };
console.log("log-message", object);
console.info("info-message", object);
console.debug("debug-message", object);
console.warn("warn-message", object);
console.error("error-message", object);
for (const [ch, subscriber] of subscribers) ch.unsubscribe(subscriber);
console.log("seen:", seen.join("|"));
console.log("mutated:", object.mutated === true);
