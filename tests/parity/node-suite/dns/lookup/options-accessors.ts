import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function options(log: string[]): any {
  const value: any = {};
  for (
    const [key, result] of [
      ["hints", 0],
      ["family", 4],
      ["all", false],
      ["verbatim", true],
      ["order", "ipv4first"],
    ] as const
  ) {
    Object.defineProperty(value, key, {
      get() {
        log.push(key);
        return result;
      },
    });
  }
  return value;
}

const callbackLog: string[] = [];
await new Promise<void>((resolve) => {
  dns.lookup("127.0.0.1", options(callbackLog), () => resolve());
});
console.log("callback access:", callbackLog.join("|"));

const promiseLog: string[] = [];
await dnsPromises.lookup("127.0.0.1", options(promiseLog));
console.log("promise access:", promiseLog.join("|"));
