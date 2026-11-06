import { randomUUID } from "node:crypto";

function isUuidV4(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(value);
}

const seen = new Set<string>();
for (let i = 0; i < 16; i++) {
  const uuid = randomUUID();
  console.log("uuid shape:", typeof uuid, uuid.length, isUuidV4(uuid));
  console.log("uuid unique:", !seen.has(uuid));
  seen.add(uuid);
}
console.log("uuid no entropy cache shape:", isUuidV4(randomUUID({ disableEntropyCache: true } as any)));
