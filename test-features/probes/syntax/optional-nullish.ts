type Nested = { child?: { value?: string } } | null;

const missing: Nested = { child: {} };
const present: Nested = { child: { value: "ok" } };

const out = [
  missing?.child?.value ?? "fallback",
  present?.child?.value ?? "fallback",
];

console.log("syntax/optional-nullish:" + out.join(","));
