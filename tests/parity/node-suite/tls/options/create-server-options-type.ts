import tls from "node:tls";

for (
  const [label, value] of [
    ["undefined", undefined],
    ["null", null],
    ["function", () => {}],
    ["string", "bad"],
    ["number", 1],
    ["boolean", true],
    ["array", []],
  ] as const
) {
  try {
    const server = value === undefined
      ? tls.createServer()
      : tls.createServer(value as any);
    console.log(
      label + ":",
      server instanceof tls.Server,
      server.listenerCount("secureConnection"),
    );
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
