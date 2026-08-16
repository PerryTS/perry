import * as z from "../../node_modules/zod/src/index.js";
import { defineApiCall, baseFields } from "./shared.js";

const Alert = z.object({
  ...baseFields(),
  severity: z.string(),
  meta: z.object({ note: z.string(), rank: z.number() }),
});

defineApiCall(
  "https://registry.example.com/v1/alerts",
  "GET",
  { paginated: true, cache: "no-store", retries: 3 },
  ["alerts", "read"],
  Alert.array(),
  (body) => "alerts:" + String((body as { id?: string }).id ?? "-"),
);

defineApiCall(
  "https://registry.example.com/v1/alerts/{alertId}",
  "GET",
  { paginated: false, cache: "no-store", retries: 1 },
  ["alerts", "read", "one"],
  Alert,
  (body) => "alert:" + String((body as { id?: string }).id ?? "-"),
);

defineApiCall(
  "https://registry.example.com/v1/alerts/{alertId}/ack",
  "POST",
  { paginated: false, cache: "no-store", retries: 0 },
  ["alerts", "write"],
  Alert,
  (body) => "ack:" + String((body as { id?: string }).id ?? "-"),
);

export { Alert };
