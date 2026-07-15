import { fork } from "node:child_process";
import { rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const helper = join(
  tmpdir(),
  `perry-child-process-serialization-${process.pid}.js`,
);
writeFileSync(
  helper,
  [
    "process.once('message', (message) => {",
    "const summary = {",
    "keys: Object.keys(message).join(','),",
    "nested: message.nested && message.nested.value,",
    "array: Array.isArray(message.items) ? message.items.join('|') : '',",
    "buffer: Buffer.isBuffer(message.buffer) ? message.buffer.toString('hex') : '',",
    "map: message.map instanceof Map ? Array.from(message.map.entries()).join('|') : '',",
    "bigint: typeof message.bigint === 'bigint' ? String(message.bigint) : '',",
    "};",
    "process.send(summary, () => process.disconnect());",
    "});",
  ].join(""),
);

async function roundTrip(serialization: "json" | "advanced", message: any) {
  const child = fork(helper, [], {
    execArgv: [],
    execPath: "node",
    serialization,
    stdio: ["ignore", "ignore", "ignore", "ipc"],
  });
  try {
    const response = await new Promise<any>((resolve, reject) => {
      child.once("error", reject);
      child.once("message", resolve);
      child.send(message);
    });
    const code = await new Promise((resolve) => child.once("close", resolve));
    console.log(`${serialization} response:`, JSON.stringify(response));
    console.log(`${serialization} close:`, code);
  } finally {
    if (child.connected) child.disconnect();
    if (child.exitCode === null) child.kill();
  }
}

try {
  await roundTrip("json", { nested: { value: 7 }, items: ["a", 2, true] });
  await roundTrip("advanced", {
    nested: { value: 8 },
    items: ["b", 3, false],
    buffer: Buffer.from([0, 127, 255]),
    map: new Map([["key", "value"]]),
    bigint: 9007199254740993n,
  });
} finally {
  rmSync(helper, { force: true });
}
