import { z } from "zod";

// ---- object schema: parse, safeParse (ok + fail), defaults, optionals ----
const User = z.object({
  name: z.string().min(2),
  age: z.number().int().nonnegative(),
  email: z.string().email().optional(),
  tags: z.array(z.string()).default([]),
  role: z.enum(["admin", "user", "guest"]),
});

const okUser = User.parse({ name: "Ada", age: 36, role: "admin" });
const badUser = User.safeParse({ name: "A", age: -1, role: "root", email: "nope" });

// ---- unions / discriminated unions ----
const Shape = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("circle"), r: z.number() }),
  z.object({ kind: z.literal("rect"), w: z.number(), h: z.number() }),
]);
const circle = Shape.parse({ kind: "circle", r: 2 });
const rectBad = Shape.safeParse({ kind: "rect", w: 3 });

// ---- transforms / refinements / pipes / coercion ----
const Slug = z.string().transform((s) => s.trim().toLowerCase().replace(/\s+/g, "-"));
const Even = z.number().refine((n) => n % 2 === 0, { message: "must be even" });
const AgeFromString = z.coerce.number().pipe(z.number().int().positive());

// ---- records / tuples / nullable ----
const Scores = z.record(z.string(), z.number());
const Pair = z.tuple([z.string(), z.number()]);
const MaybeName = z.string().nullable();

// ---- nested + partial-ish ----
const Config = z.object({
  server: z.object({ host: z.string(), port: z.number().default(8080) }),
  flags: z.array(z.boolean()),
});

const out = {
  okUser,
  badOk: badUser.success,
  badPaths: badUser.success ? [] : badUser.error.issues.map((i) => i.path.join(".")).sort(),
  badCount: badUser.success ? 0 : badUser.error.issues.length,
  circle,
  rectBadOk: rectBad.success,
  slug: Slug.parse("  Hello World Foo  "),
  evenOk: Even.safeParse(10).success,
  evenBad: Even.safeParse(7).success,
  ageFromStr: AgeFromString.parse("42"),
  scores: Scores.parse({ a: 1, b: 2 }),
  pair: Pair.parse(["x", 9]),
  maybeNull: MaybeName.parse(null),
  config: Config.parse({ server: { host: "h" }, flags: [true, false] }),
  isZodError: !badUser.success && badUser.error instanceof z.ZodError,
};

console.log(JSON.stringify(out));
