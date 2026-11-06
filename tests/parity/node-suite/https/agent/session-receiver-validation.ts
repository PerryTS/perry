import * as https from "node:https";

function check(name: string) {
  const method = (https.Agent.prototype as any)[name];
  console.log(name, "typeof:", typeof method);
  if (typeof method !== "function") return;
  try {
    method.call({}, "key", Buffer.from("value"));
    console.log(name, "accepted");
  } catch (error: any) {
    console.log(name, error.name);
  }
}

check("_getSession");
check("_cacheSession");
check("_evictSession");
