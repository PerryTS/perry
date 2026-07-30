import dns from "node:dns";
import dnsPromises from "node:dns/promises";

function callback(port: any): Promise<any> {
  return new Promise((resolve) => {
    dns.lookupService("127.0.0.1", port, (error, hostname, service) => {
      resolve(error ? { error: error.code } : { hostname, service });
    });
  }).catch((error: any) => ({ error: error.code }));
}

function summary(value: any): string {
  return value.error
    ? `error:${value.error}`
    : `${typeof value.hostname}/${typeof value.service}`;
}

const callbackNumber = await callback(22);
const callbackString = await callback("22");
console.log(
  "callback equal:",
  JSON.stringify(callbackNumber) === JSON.stringify(callbackString),
  summary(callbackString),
);

async function promise(port: any): Promise<any> {
  try {
    return await (dnsPromises.lookupService as any)("127.0.0.1", port);
  } catch (error: any) {
    return { error: error.code };
  }
}

const promiseNumber = await promise(22);
const promiseString = await promise("22");
console.log(
  "promise equal:",
  JSON.stringify(promiseNumber) === JSON.stringify(promiseString),
  summary(promiseString),
);
