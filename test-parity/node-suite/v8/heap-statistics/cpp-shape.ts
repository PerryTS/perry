import { getCppHeapStatistics } from "node:v8";

const expected = [
  "committed_size_bytes",
  "detail_level",
  "resident_size_bytes",
  "space_statistics",
  "type_names",
  "used_size_bytes",
];
for (const mode of [undefined, "brief", "detailed"] as const) {
  const stats: any = getCppHeapStatistics(mode);
  console.log("mode:", mode ?? "default", stats.detail_level);
  console.log(
    "keys:",
    Object.keys(stats).sort().join(",") === expected.join(","),
  );
  console.log(
    "numbers:",
    [
      stats.committed_size_bytes,
      stats.resident_size_bytes,
      stats.used_size_bytes,
    ].every((value) =>
      typeof value === "number" && Number.isFinite(value) && value >= 0
    ),
  );
  console.log(
    "arrays:",
    Array.isArray(stats.space_statistics),
    Array.isArray(stats.type_names),
  );
}

for (const value of ["invalid", 1, null] as const) {
  try {
    getCppHeapStatistics(value as any);
    console.log("invalid:", String(value), "no throw");
  } catch (error: any) {
    console.log("invalid:", String(value), error.name, error.code);
  }
}
