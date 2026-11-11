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
    "set: message.set instanceof Set ? Array.from(message.set.values()).join('|') : '',",
    "date: message.date instanceof Date ? message.date.toISOString() : '',",
    "regexp: message.regexp instanceof RegExp ? message.regexp.toString() : '',",
    "uint16: message.uint16 instanceof Uint16Array ? Array.from(message.uint16).join('|') : '',",
    "emptyBuffer: Buffer.isBuffer(message.emptyBuffer) ? String(message.emptyBuffer.length) : '',",
    "emptyUint8: message.emptyUint8 instanceof Uint8Array ? String(message.emptyUint8.length) : '',",
    "bigint: typeof message.bigint === 'bigint' ? String(message.bigint) : '',",
    "error: message.error instanceof TypeError ? message.error.message : '',",
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
    set: new Set(["first", "second"]),
    date: new Date("2020-01-02T03:04:05.000Z"),
    regexp: /child-process/gi,
    uint16: new Uint16Array([1, 256, 65535]),
    emptyBuffer: Buffer.alloc(0),
    emptyUint8: new Uint8Array(0),
    bigint: 9007199254740993n,
    error: new TypeError("advanced-error"),
  });
} finally {
  rmSync(helper, { force: true });
}
