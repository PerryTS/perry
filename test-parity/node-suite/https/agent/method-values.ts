import * as http from "node:http";
import * as https from "node:https";

const options = {
  host: "example.com",
  port: 443,
  ca: "ca",
  rejectUnauthorized: false,
  servername: "sni.example",
};

function logMethod(label: string, agent: any, name: string) {
  const fn = agent[name];
  console.log(`${label}.${name} typeof:`, typeof fn);
  console.log(`${label}.${name} name:`, fn && fn.name);
  console.log(`${label}.${name} length:`, fn && fn.length);
}

const agent: any = new https.Agent({ keepAlive: true, maxSockets: 5 });
for (const name of ["keepSocketAlive", "reuseSocket", "getName", "destroy"]) {
  logMethod("agent", agent, name);
}

const captured = agent.getName;
console.log("agent captured typeof:", typeof captured);
console.log("agent captured name:", captured && captured.name);
console.log("agent captured length:", captured && captured.length);
console.log("agent captured direct getName:", captured(options));
console.log("agent captured getName:", captured.call(agent, options));
console.log("agent direct getName:", agent.getName(options));

for (const name of ["keepSocketAlive", "reuseSocket", "getName", "destroy"]) {
  logMethod("https.globalAgent", https.globalAgent as any, name);
}

const globalGetName = (https.globalAgent as any).getName;
console.log("https.globalAgent captured getName:", globalGetName(options));
console.log("https.globalAgent call getName:", globalGetName.call({ protocol: "http:" }, options));
console.log("https.globalAgent direct getName:", (https.globalAgent as any).getName(options));

const httpGetName = (http.globalAgent as any).getName;
console.log("http.globalAgent captured getName:", httpGetName({ host: "example.com", port: 80 }));
