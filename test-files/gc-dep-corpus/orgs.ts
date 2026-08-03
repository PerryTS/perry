import * as z from "../../node_modules/zod/src/index.js";
import { defineApiCall, baseFields } from "./shared.js";

const Org = z.object({
  ...baseFields(),
  slug: z.string().min(2),
  seats: z.number().int(),
  owners: z.array(z.object({ login: z.string(), role: z.string() })),
});

defineApiCall(
  "https://registry.example.com/v1/orgs",
  "GET",
  { paginated: true, cache: "default", retries: 2 },
  ["orgs", "read"],
  Org.array(),
  (body) => "orgs:" + String((body as { slug?: string }).slug ?? "-"),
);

defineApiCall(
  "https://registry.example.com/v1/orgs/{orgId}/members",
  "GET",
  { paginated: true, cache: "default", retries: 2 },
  ["orgs", "read", "members"],
  Org,
  (body) => "members:" + String((body as { slug?: string }).slug ?? "-"),
);

defineApiCall(
  "https://registry.example.com/v1/orgs/{orgId}/seats",
  "PUT",
  { paginated: false, cache: "no-store", retries: 0 },
  ["orgs", "write"],
  Org,
  (body) => "seats:" + String((body as { slug?: string }).slug ?? "-"),
);

export { Org };
