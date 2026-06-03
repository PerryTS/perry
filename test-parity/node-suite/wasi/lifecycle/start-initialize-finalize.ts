import { WASI } from "node:wasi";

const W: any = WASI;

const commandBytes = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2,
  1, 0, 5, 3, 1, 0, 1, 7, 19, 2, 6, 109, 101, 109, 111, 114,
  121, 2, 0, 6, 95, 115, 116, 97, 114, 116, 0, 0, 10, 4, 1,
  2, 0, 11,
]);

const reactorBytes = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2,
  1, 0, 5, 3, 1, 0, 1, 7, 24, 2, 6, 109, 101, 109, 111, 114,
  121, 2, 0, 11, 95, 105, 110, 105, 116, 105, 97, 108, 105,
  122, 101, 0, 0, 10, 4, 1, 2, 0, 11,
]);

const bothBytes = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 3,
  2, 0, 0, 5, 3, 1, 0, 1, 7, 33, 3, 6, 109, 101, 109, 111,
  114, 121, 2, 0, 6, 95, 115, 116, 97, 114, 116, 0, 0, 11,
  95, 105, 110, 105, 116, 105, 97, 108, 105, 122, 101, 0, 1,
  10, 8, 2, 3, 0, 0, 11, 2, 0, 11,
]);

const memoryOnlyBytes = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 5, 3, 1, 0, 1, 7, 10, 1,
  6, 109, 101, 109, 111, 114, 121, 2, 0,
]);

type InstanceLike = { exports: Record<string, any> };

function fallbackCommand(): InstanceLike {
  return { exports: { memory: {}, _start() {} } };
}

function fallbackReactor(): InstanceLike {
  return { exports: { memory: {}, _initialize() {} } };
}

function fallbackBoth(): InstanceLike {
  return { exports: { memory: {}, _start() {}, _initialize() {} } };
}

function fallbackMemoryOnly(): InstanceLike {
  return { exports: { memory: {} } };
}

async function instanceOrFallback(
  bytes: Uint8Array,
  fallback: () => InstanceLike,
): Promise<any> {
  const WA: any = (globalThis as any)["WebAssembly"];
  const instantiate = WA?.["instantiate"];
  try {
    if (typeof instantiate === "function") {
      const result: any = await instantiate.call(WA, bytes, {});
      const instance = result?.instance ?? result;
      if (
        instance &&
        typeof instance === "object" &&
        typeof instance.exports === "object"
      ) {
        return instance;
      }
    }
  } catch {}
  return fallback();
}

function errorTag(err: any) {
  const message = String(err?.message ?? "");
  if (message.includes("already started")) return "already-started";
  if (message.includes("instance.exports._start") && message.includes("of type function")) {
    return "_start-function";
  }
  if (message.includes("instance.exports._start") && message.includes("must be undefined")) {
    return "_start-undefined";
  }
  if (message.includes("instance.exports._initialize")) return "_initialize-undefined";
  if (message.includes("instance.exports.memory")) return "memory";
  if (message.includes("instance.exports")) return "exports-object";
  if (message.includes("\"instance\" argument")) return "instance-object";
  return "other";
}

function show(label: string, fn: () => any) {
  try {
    console.log(`${label}: ok`, String(fn()));
  } catch (err: any) {
    console.log(`${label}: throw`, err?.name, err?.code || "no-code", errorTag(err));
  }
}

const command = await instanceOrFallback(commandBytes, fallbackCommand);
const reactor = await instanceOrFallback(reactorBytes, fallbackReactor);
const both = await instanceOrFallback(bothBytes, fallbackBoth);
const memoryOnly = await instanceOrFallback(memoryOnlyBytes, fallbackMemoryOnly);

const startedByStart = new W({ version: "preview1", returnOnExit: true });
console.log("own keys before:", Object.keys(startedByStart).join(","));
show("start command", () => startedByStart.start(command));
console.log("own keys after:", Object.keys(startedByStart).join(","));
show("start command again", () => startedByStart.start(command));
show("initialize after start", () => startedByStart.initialize(reactor));
show("finalize after start", () => startedByStart.finalizeBindings(command));

const startedByInitialize = new W({ version: "preview1", returnOnExit: true });
show("initialize reactor", () => startedByInitialize.initialize(reactor));
show("initialize reactor again", () => startedByInitialize.initialize(reactor));
show("start after initialize", () => startedByInitialize.start(command));

show("start reactor fresh", () =>
  new W({ version: "preview1", returnOnExit: true }).start(reactor)
);
show("initialize command fresh", () =>
  new W({ version: "preview1", returnOnExit: true }).initialize(command)
);
show("start both fresh", () =>
  new W({ version: "preview1", returnOnExit: true }).start(both)
);
show("initialize memory-only fresh", () =>
  new W({ version: "preview1", returnOnExit: true }).initialize(memoryOnly)
);
show("start memory-only fresh", () =>
  new W({ version: "preview1", returnOnExit: true }).start(memoryOnly)
);
show("finalize reactor fresh", () =>
  new W({ version: "preview1", returnOnExit: true }).finalizeBindings(reactor)
);

const startedByFinalize = new W({ version: "preview1", returnOnExit: true });
show("finalize command", () => startedByFinalize.finalizeBindings(command));
show("start after finalize", () => startedByFinalize.start(command));

for (const [label, value] of [
  ["undefined", undefined],
  ["null", null],
  ["number", 1],
  ["plain object", {}],
] as const) {
  show(`initialize input ${label}`, () =>
    new W({ version: "preview1", returnOnExit: true }).initialize(value)
  );
}
