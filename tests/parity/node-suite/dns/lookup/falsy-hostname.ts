import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function callbackShape(hostname: any): string {
  try {
    dns.lookup(hostname, () => {});
    return "no throw";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

async function promiseShape(hostname: any): Promise<string> {
  try {
    await dnsPromises.lookup(hostname);
    return "resolved";
  } catch (error: any) {
    return `${error.name}/${error.code}`;
  }
}

for (const value of [undefined, null, "", 0, false, NaN]) {
  console.log(
    String(value) + ":",
    callbackShape(value),
    await promiseShape(value),
  );
}
