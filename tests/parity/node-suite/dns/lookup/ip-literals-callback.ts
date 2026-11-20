import dns from "node:dns";

function lookup(hostname: string, options?: any): Promise<any[]> {
  return new Promise((resolve) => {
    const callback = (...args: any[]) => resolve(args);
    const request = options === undefined
      ? dns.lookup(hostname, callback)
      : dns.lookup(hostname, options, callback);
    console.log("request object:", typeof request, request !== null);
  });
}

for (
  const [hostname, options] of [
    ["127.0.0.1", undefined],
    ["::1", undefined],
    ["127.0.0.1", { all: true }],
  ] as const
) {
  const [error, value, family] = await lookup(hostname, options);
  console.log(
    hostname + ":",
    error === null,
    JSON.stringify(value),
    String(family),
  );
}
