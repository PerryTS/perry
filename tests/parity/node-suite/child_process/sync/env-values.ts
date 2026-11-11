import { spawnSync } from "node:child_process";

const result = spawnSync(
  "node",
  [
    "-e",
    "process.stdout.write(JSON.stringify({number:process.env.PERRY_NUMBER,boolean:process.env.PERRY_BOOLEAN,nullValue:process.env.PERRY_NULL,empty:process.env.PERRY_EMPTY,missing:Object.hasOwn(process.env,'PERRY_UNDEFINED')}))",
  ],
  {
    encoding: "utf8",
    env: {
      ...process.env,
      PERRY_NUMBER: 42 as any,
      PERRY_BOOLEAN: false as any,
      PERRY_NULL: null as any,
      PERRY_EMPTY: "",
      PERRY_UNDEFINED: undefined,
    },
  },
);
console.log("status:", result.status);
console.log("stdout:", result.stdout);
console.log("stderr:", JSON.stringify(result.stderr));
