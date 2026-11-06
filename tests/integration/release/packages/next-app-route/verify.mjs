const base = process.env.BASE_URL ?? "http://127.0.0.1:3100";

function checksum(iterations) {
  let value = 0x811c9dc5;
  for (let index = 0; index < iterations; index += 1) {
    value = Math.imul(value ^ index, 0x01000193) >>> 0;
  }
  return value;
}

async function verify(id, iterations, method = "GET", requestBody = "") {
  const response = await fetch(
    `${base}/api/benchmark?id=${encodeURIComponent(id)}&iterations=${iterations}`,
    {
      method,
      headers: {
        "x-request-id": id,
        ...(method === "POST" ? { "content-type": "text/plain" } : {}),
      },
      ...(method === "POST" ? { body: requestBody } : {}),
    },
  );
  const body = await response.json();
  const cookie = response.headers.get("set-cookie") ?? "";
  const expected = {
    runtime: "next",
    method,
    pathname: "/api/benchmark",
    id,
    iterations,
    checksum: checksum(iterations),
    beforeAwait: id,
    afterAwait: id,
    requestBody,
  };
  if (response.status !== 207) {
    throw new Error(`${id}: status ${response.status}`);
  }
  if (response.headers.get("x-perry-repro") !== id) {
    throw new Error(`${id}: response header lost`);
  }
  if (!cookie.includes(`perry_ctx=${id}`)) {
    throw new Error(`${id}: response cookie lost`);
  }
  if (JSON.stringify(body) !== JSON.stringify(expected)) {
    throw new Error(
      `${id}: ${JSON.stringify(body)} != ${JSON.stringify(expected)}`,
    );
  }
}

await Promise.all(
  Array.from({ length: 20 }, (_, index) =>
    verify(`request-${index}`, index + 1),
  ),
);
await verify("post-request", 31, "POST", "perry-request-body");
console.log("PASS: 21 production App Route requests");
