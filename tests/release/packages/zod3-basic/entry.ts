import { z } from "zod";

function print(label: string, value: unknown): void {
  console.log(`${label}=${JSON.stringify(value)}`);
}

function summarizeIssues(error: z.ZodError): { path: string; code: string; message: string }[] {
  return error.issues.map((issue) => ({
    path: issue.path.join("."),
    code: issue.code,
    message: issue.message,
  }));
}

const userSchema = z.object({
  id: z.number().int().positive(),
  name: z.string().min(2).transform((value) => value.trim().toUpperCase()),
  tags: z.array(z.string()).default([]),
  role: z.enum(["admin", "user"]).default("user"),
  meta: z.object({ active: z.boolean(), score: z.number().optional() }).passthrough(),
});

const parsed = userSchema.parse({
  id: 7,
  name: " ada ",
  meta: { active: true, source: "seed" },
});
print("parse", parsed);

const badUser = userSchema.safeParse({
  id: -1,
  name: "x",
  tags: ["ok", 2],
  meta: { active: "yes" },
});
print("safeParse.success", badUser.success);
if (!badUser.success) {
  print("safeParse.issues", summarizeIssues(badUser.error));
}

print("primitives", {
  string: z.string().parse("x"),
  number: z.number().parse(1.5),
  int: z.number().int().safeParse(2.2).success,
  boolean: z.boolean().parse(false),
  null: z.null().parse(null),
  undefined: z.undefined().parse(undefined) === undefined,
  any: z.any().parse({ a: 1 }).a,
  unknown: z.unknown().parse([1, 2]).length,
  never: z.never().safeParse("nope").success,
});

print("coerce", {
  string: z.coerce.string().parse(42),
  number: z.coerce.number().parse("12.5"),
  boolean: z.coerce.boolean().parse(1),
  date: z.coerce.date().parse("2020-01-02T00:00:00.000Z").toISOString(),
});

print("literals.enums", {
  literal: z.literal("ready").safeParse("ready").success,
  literalFail: z.literal(3).safeParse(4).success,
  enum: z.enum(["red", "blue"]).parse("blue"),
  nativeEnum: z.nativeEnum({ A: "a", B: "b" } as const).parse("a"),
});

const stringSchema = z
  .string()
  .min(3)
  .max(8)
  .regex(/^[a-z-]+$/)
  .startsWith("ab")
  .endsWith("yz")
  .includes("-")
  .trim()
  .toUpperCase();
print("strings", {
  value: stringSchema.parse("ab-yz"),
  email: z.string().email().safeParse("a@example.com").success,
  uuid: z.string().uuid().safeParse("550e8400-e29b-41d4-a716-446655440000").success,
  url: z.string().url().safeParse("https://example.com/a?b=1").success,
  datetime: z.string().datetime().safeParse("2020-01-02T03:04:05.000Z").success,
  ip: z.string().ip().safeParse("127.0.0.1").success,
});

print("numbers", {
  finite: z.number().finite().safeParse(Number.POSITIVE_INFINITY).success,
  gt: z.number().gt(1).safeParse(2).success,
  gte: z.number().gte(2).safeParse(2).success,
  lt: z.number().lt(3).safeParse(3).success,
  lte: z.number().lte(3).safeParse(3).success,
  multiple: z.number().multipleOf(5).safeParse(15).success,
});

const eventSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("text"), value: z.string() }),
  z.object({ type: z.literal("count"), value: z.number().int() }),
]);
print("discriminatedUnion", [
  eventSchema.parse({ type: "text", value: "hello" }),
  eventSchema.parse({ type: "count", value: 3 }),
]);

const primitiveSchema = z.union([z.string().regex(/^id-/), z.number().int()]);
print("union", [primitiveSchema.safeParse("id-42").success, primitiveSchema.safeParse(4.5).success]);

const objectBase = z.object({ id: z.number(), name: z.string(), active: z.boolean().optional() });
print("objects", {
  strip: objectBase.parse({ id: 1, name: "a", extra: true } as unknown),
  strict: objectBase.strict().safeParse({ id: 1, name: "a", extra: true }).success,
  passthrough: objectBase.passthrough().parse({ id: 1, name: "a", extra: true } as unknown),
  catchall: z.object({ id: z.number() }).catchall(z.string()).safeParse({ id: 1, extra: "ok" }).success,
  extend: objectBase.extend({ role: z.literal("admin") }).parse({ id: 1, name: "a", role: "admin" }),
  pick: objectBase.pick({ id: true }).parse({ id: 1 }),
  omit: objectBase.omit({ active: true }).parse({ id: 1, name: "a" }),
  partial: objectBase.partial().parse({ id: 1 }),
  required: objectBase.required().safeParse({ id: 1, name: "a" }).success,
});

print("arrays.tuples", {
  array: z.array(z.number()).min(2).max(3).parse([1, 2]),
  nonempty: z.array(z.string()).nonempty().safeParse([]).success,
  tuple: z.tuple([z.string(), z.number()]).parse(["a", 1]),
  tupleRest: z.tuple([z.string()]).rest(z.number()).parse(["a", 1, 2]),
});

print("collections", {
  record: z.record(z.number()).safeParse({ a: 1, b: 2 }).success,
  recordFail: z.record(z.number()).refine((scores) => Object.values(scores).every((score) => score >= 0)).safeParse({ a: 1, b: -1 }).success,
});

print("composition", {
  intersection: z.intersection(z.object({ a: z.string() }), z.object({ b: z.number() })).parse({ a: "x", b: 1 }),
  or: z.string().or(z.number()).parse(5),
  and: z.object({ a: z.string() }).and(z.object({ b: z.boolean() })).parse({ a: "x", b: true }),
});

const csvSchema = z
  .string()
  .transform((value) => value.split(",").map((item) => item.trim()).filter(Boolean))
  .refine((items) => items.length >= 2, { message: "need two values" });
print("transform", csvSchema.parse("a, b, c"));
print("refine", csvSchema.safeParse("single").success);

const preprocessSchema = z.preprocess((value) => (typeof value === "string" ? value.trim() : value), z.string().min(2));
const superRefineSchema = z.array(z.number()).superRefine((values, ctx) => {
  if (new Set(values).size !== values.length) {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: "duplicates" });
  }
});
print("effects", {
  preprocess: preprocessSchema.parse(" ok "),
  pipe: z.string().transform((value) => value.length).pipe(z.number().min(2)).safeParse("abc").success,
  superRefine: superRefineSchema.safeParse([1, 1]).success,
  custom: z.custom<string>((value) => value === "token").safeParse("token").success,
});

print("modifiers", {
  optional: z.string().optional().parse(undefined) === undefined,
  nullable: z.string().nullable().parse(null) === null,
  nullish: z.string().nullish().parse(undefined) === undefined,
  default: z.string().default("fallback").parse(undefined),
  catch: z.number().catch(9).parse("bad"),
  readonlyFrozen: Object.isFrozen(z.object({ id: z.number() }).readonly().parse({ id: 1 })),
});

type Tree = { name: string; children?: Tree[] };
const treeSchema: z.ZodType<Tree> = z.lazy(() => z.object({ name: z.string(), children: z.array(treeSchema).optional() }));
print("lazy", treeSchema.parse({ name: "root", children: [{ name: "leaf" }] }));

class Box {
  value: string;
  constructor(value: string) {
    this.value = value;
  }
}
print("instances.dates", {
  instanceof: z.instanceof(Box).parse(new Box("ok")).value,
  date: z.date().parse(new Date("2020-01-02T00:00:00.000Z")).toISOString(),
});

const validatedFn = z.function().args(z.string()).returns(z.number()).implement((value) => value.trim().length);
print("function", {
  valid: validatedFn(" tuna "),
  invalidArgs: (() => {
    try {
      (validatedFn as unknown as (value: number) => number)(1);
      return false;
    } catch {
      return true;
    }
  })(),
});

const asyncSchema = z.string().refine(async (value) => value === "ok");
print("async", await asyncSchema.safeParseAsync("ok"));

const formatted = objectBase.safeParse({ id: "x", name: 1 });
if (!formatted.success) {
  print("errors", {
    formatKeys: Object.keys(formatted.error.format()).sort(),
    flatten: formatted.error.flatten(),
  });
}

const summarySchema = z.array(userSchema.pick({ id: true, role: true })).min(1);
print("array", summarySchema.parse([{ id: 1, role: "admin" }]));
