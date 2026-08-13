import * as z from "../../node_modules/zod/src/index.js";
import { defineApiCall, baseFields } from "./shared.js";

const Scan = z.object({
  ...baseFields(),
  digest: z.string().regex(/^[a-f0-9]{8,}$/),
  findings: z.array(
    z.object({ rule: z.string(), level: z.string(), line: z.number().int() }),
  ),
  summary: z.union([z.string(), z.number()]),
});

defineApiCall(
  "https://registry.example.com/v1/scans",
  "POST",
  { paginated: false, cache: "no-store", retries: 0, timeoutMs: 30000 },
  ["scans", "write"],
  Scan,
  (body) => "scan:" + String((body as { digest?: string }).digest ?? "-"),
);

defineApiCall(
  "https://registry.example.com/v1/scans/{scanId}",
  "GET",
  { paginated: false, cache: "default", retries: 2 },
  ["scans", "read"],
  Scan,
  (body) => "scanone:" + String((body as { digest?: string }).digest ?? "-"),
);

defineApiCall(
  "https://registry.example.com/v1/scans/{scanId}/findings",
  "GET",
  { paginated: true, cache: "default", retries: 2 },
  ["scans", "read", "findings"],
  Scan.array(),
  (body) => "findings:" + String((body as { digest?: string }).digest ?? "-"),
);

export { Scan };
