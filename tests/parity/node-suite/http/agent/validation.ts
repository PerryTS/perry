import { Agent } from "node:http";

function check(label: string, options: any) {
  try {
    new Agent(options).destroy();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

check("scheduling", { scheduling: "random" });
check("max total zero", { maxTotalSockets: 0 });
check("max total string", { maxTotalSockets: "2" });
check("buffer negative fallback", { agentKeepAliveTimeoutBuffer: -1 });
