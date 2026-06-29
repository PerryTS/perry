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
  bigint: z.bigint().parse(10n).toString(),
  symbol: typeof z.symbol().parse(Symbol.for("fixture")),
  nan: Number.isNaN(z.nan().parse(NaN)),
  void: z.void().parse(undefined) === undefined,
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
  bigint: z.coerce.bigint().parse("42").toString(),
  date: z.coerce.date().parse("2020-01-02T00:00:00.000Z").toISOString(),
});

const colorEnum = z.enum(["red", "blue", "green"]);
print("literals.enums", {
  literal: z.literal("ready").safeParse("ready").success,
  literalBigint: z.literal(2n).safeParse(2n).success,
  literalBoolean: z.literal(true).safeParse(false).success,
  literalNull: z.literal(null).parse(null),
  literalFail: z.literal(3).safeParse(4).success,
  enum: colorEnum.parse("blue"),
  enumBlue: colorEnum.enum.blue,
  enumValues: colorEnum.Values.green,
  enumEnum: colorEnum.Enum.red,
  enumOptions: colorEnum.options.join("|"),
  enumKeys: Object.keys(colorEnum.enum).sort(),
  enumExtract: colorEnum.extract(["red", "green"]).safeParse("blue").success,
  enumExtractOk: colorEnum.extract(["red", "green"]).parse("red"),
  enumExclude: colorEnum.exclude(["blue"]).parse("green"),
  nativeEnum: z.nativeEnum({ A: "a", B: "b" } as const).parse("a"),
  nativeEnumNumber: z.nativeEnum({ A: 1, B: 2 } as const).parse(2),
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
  datetimeOffset: z.string().datetime({ offset: true }).safeParse("2020-01-02T03:04:05+02:00").success,
  datetimePrecision: z.string().datetime({ precision: 3 }).safeParse("2020-01-02T03:04:05.123Z").success,
  datetimeLocal: z.string().datetime({ local: true }).safeParse("2020-01-02T03:04:05").success,
  ip: z.string().ip().safeParse("127.0.0.1").success,
  ipv4: z.string().ip({ version: "v4" }).safeParse("127.0.0.1").success,
  ipv6: z.string().ip({ version: "v6" }).safeParse("::1").success,
  length: z.string().length(3).safeParse("abc").success,
  nonempty: z.string().nonempty().safeParse("").success,
  timePrecision: z.string().time({ precision: 0 }).safeParse("03:04:05.123").success,
  includesPosition: z.string().includes("b", { position: 1 }).safeParse("abc").success,
  includesPositionFail: z.string().includes("b", { position: 2 }).safeParse("abc").success,
  lowercase: z.string().toLowerCase().parse("ABC"),
  cuid: z.string().cuid().safeParse("ckj8lp2e90000v4j5x6j8s9abc").success,
  cuid2: z.string().cuid2().safeParse("tz4a98xxat96iws9zmbrgj3a").success,
  ulid: z.string().ulid().safeParse("01ARZ3NDEKTSV4RRFFQ69G5FAV").success,
  emoji: z.string().emoji().safeParse("😀").success,
  nanoid: z.string().nanoid().safeParse("V1StGXR8_Z5jdHi6B-myT").success,
  base64: z.string().base64().safeParse("aGVsbG8=").success,
  base64url: z.string().base64url().safeParse("aGVsbG8").success,
  jwt: z.string().jwt().safeParse("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature").success,
  date: z.string().date().safeParse("2020-01-02").success,
  time: z.string().time().safeParse("03:04:05").success,
  duration: z.string().duration().safeParse("P1Y2M3DT4H5M6S").success,
  cidr: z.string().cidr().safeParse("192.168.0.0/24").success,
  cidrv4: z.string().cidr({ version: "v4" }).safeParse("192.168.0.0/24").success,
  cidrv6: z.string().cidr({ version: "v6" }).safeParse("2001:db8::/32").success,
  regexFail: z.string().regex(/^[a-z]+$/).safeParse("abc1").success,
});

print("numbers", {
  finite: z.number().finite().safeParse(Number.POSITIVE_INFINITY).success,
  min: z.number().min(2).safeParse(2).success,
  max: z.number().max(2).safeParse(3).success,
  gt: z.number().gt(1).safeParse(2).success,
  gte: z.number().gte(2).safeParse(2).success,
  lt: z.number().lt(3).safeParse(3).success,
  lte: z.number().lte(3).safeParse(3).success,
  multiple: z.number().multipleOf(5).safeParse(15).success,
  step: z.number().step(0.5).safeParse(1.5).success,
  positive: z.number().positive().safeParse(1).success,
  nonpositive: z.number().nonpositive().safeParse(0).success,
  negative: z.number().negative().safeParse(-1).success,
  nonnegative: z.number().nonnegative().safeParse(0).success,
  safe: z.number().safe().safeParse(Number.MAX_SAFE_INTEGER + 1).success,
});

print("bigints", {
  min: z.bigint().min(2n).safeParse(2n).success,
  max: z.bigint().max(2n).safeParse(3n).success,
  gt: z.bigint().gt(1n).safeParse(2n).success,
  gte: z.bigint().gte(2n).safeParse(2n).success,
  lt: z.bigint().lt(3n).safeParse(3n).success,
  lte: z.bigint().lte(3n).safeParse(3n).success,
  multiple: z.bigint().multipleOf(5n).safeParse(15n).success,
  positive: z.bigint().positive().safeParse(1n).success,
  nonpositive: z.bigint().nonpositive().safeParse(0n).success,
});

const eventSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("text"), value: z.string() }),
  z.object({ type: z.literal("count"), value: z.number().int() }),
]);
print("discriminatedUnion", [
  eventSchema.parse({ type: "text", value: "hello" }),
  eventSchema.parse({ type: "count", value: 3 }),
]);
print("discriminatedUnion.options", eventSchema.options.length);

const primitiveSchema = z.union([z.string().regex(/^id-/), z.number().int()]);
print("union", {
  results: [primitiveSchema.safeParse("id-42").success, primitiveSchema.safeParse(4.5).success],
  options: primitiveSchema.options.length,
});

const objectBase = z.object({ id: z.number(), name: z.string(), active: z.boolean().optional() });
const nestedObject = z.object({ nested: z.object({ label: z.string() }) });
const categorySchema: z.ZodType<any> = z.object({
  name: z.string(),
  get subcategories() {
    return z.array(categorySchema);
  },
});
const authorSchema: z.ZodType<any> = z.object({
  email: z.string().email(),
  get posts() {
    return z.array(postSchema);
  },
});
const postSchema: z.ZodType<any> = z.object({
  title: z.string(),
  get author() {
    return authorSchema;
  },
});
print("objects", {
  strip: objectBase.parse({ id: 1, name: "a", extra: true } as unknown),
  explicitStrip: objectBase.strip().parse({ id: 1, name: "a", extra: true } as unknown),
  strict: objectBase.strict().safeParse({ id: 1, name: "a", extra: true }).success,
  strictObject: z.strictObject({ name: z.string() }).safeParse({ name: "a", extra: true }).success,
  nonstrict: objectBase.nonstrict().parse({ id: 1, name: "a", extra: true } as unknown),
  passthrough: objectBase.passthrough().parse({ id: 1, name: "a", extra: true } as unknown),
  catchall: z.object({ id: z.number() }).catchall(z.string()).safeParse({ id: 1, extra: "ok" }).success,
  catchallFail: z.object({ id: z.number() }).catchall(z.string()).safeParse({ id: 1, extra: 1 }).success,
  extend: objectBase.extend({ role: z.literal("admin") }).parse({ id: 1, name: "a", role: "admin" }),
  augment: z.object({ id: z.number() }).augment({ name: z.string() }).parse({ id: 1, name: "a" }),
  setKey: z.object({ id: z.number() }).setKey("name", z.string()).parse({ id: 1, name: "a" }),
  merge: objectBase.merge(z.object({ role: z.string() })).parse({ id: 1, name: "a", role: "user" }),
  strictMessage: z.object({ id: z.number() }).strict("no extras").safeParse({ id: 1, extra: true }).success,
  keyof: objectBase.keyof().parse("name"),
  shapeName: objectBase.shape.name.safeParse("a").success,
  pick: objectBase.pick({ id: true }).parse({ id: 1 }),
  omit: objectBase.omit({ active: true }).parse({ id: 1, name: "a" }),
  partial: objectBase.partial().parse({ id: 1 }),
  partialName: objectBase.partial({ name: true }).safeParse({ id: 1 }).success,
  deepPartial: nestedObject.deepPartial().parse({ nested: {} }),
  required: objectBase.required().safeParse({ id: 1, name: "a" }).success,
  requiredActive: objectBase.required({ active: true }).safeParse({ id: 1, name: "a" }).success,
  nestedRequired: nestedObject.required().safeParse({ nested: { label: "x" } }).success,
});

print("recursiveObjects", {
  category: categorySchema.parse({ name: "root", subcategories: [{ name: "leaf", subcategories: [] }] }),
  mutual: postSchema.parse({ title: "post", author: { email: "a@example.com", posts: [] } }).author.email,
});

const arrayElementIssue = z.array(z.string()).safeParse([1]);
const tupleLengthIssue = z.tuple([z.string()]).safeParse(["a", "b"]);
print("arrays.tuples", {
  array: z.array(z.number()).min(2).max(3).parse([1, 2]),
  transformedArray: z.array(z.string().transform((value) => value.length)).parse(["aa", "bbb"]),
  exactLength: z.array(z.string()).length(2).safeParse(["a", "b"]).success,
  nonempty: z.array(z.string()).nonempty().safeParse([]).success,
  elementDescription: z.array(z.string().describe("array item")).element.description,
  elementIssuePath: arrayElementIssue.success ? "ok" : arrayElementIssue.error.issues[0].path.join("."),
  tuple: z.tuple([z.string(), z.number()]).parse(["a", 1]),
  tupleLengthIssue: tupleLengthIssue.success ? "ok" : tupleLengthIssue.error.issues[0].code,
  tupleRest: z.tuple([z.string()]).rest(z.number()).parse(["a", 1, 2]),
});

const parsedMap = z.map(z.string(), z.number()).parse(new Map([["a", 1], ["b", 2]]));
const parsedSet = z.set(z.string()).min(2).parse(new Set(["a", "b"]));
const transformedMap = z
  .map(z.string(), z.string().transform((value) => value.length))
  .parse(new Map([["size", "abcd"]]));
const transformedSet = z
  .set(z.string().transform((value) => value.length))
  .parse(new Set(["aa", "bbb"]));
print("collections", {
  record: z.record(z.number()).safeParse({ a: 1, b: 2 }).success,
  keyedRecord: z.record(z.enum(["a", "b"]), z.number()).safeParse({ a: 1, b: 2 }).success,
  keyedRecordMissing: z.record(z.enum(["a", "b"]), z.number()).safeParse({ a: 1 }).success,
  stringKeyRecord: z.record(z.string().min(1), z.number()).safeParse({ "": 1 }).success,
  transformedRecord: z
    .record(z.string().transform((value) => value.length))
    .parse({ a: "abcd" }),
  recordFail: z.record(z.number()).refine((scores) => Object.values(scores).every((score) => score >= 0)).safeParse({ a: 1, b: -1 }).success,
  map: Array.from(parsedMap.entries()),
  mapKeyFail: z.map(z.string().min(2), z.number()).safeParse(new Map([["a", 1]])).success,
  mapFail: z.map(z.string(), z.number()).safeParse(new Map([["bad", "x"]])).success,
  transformedMap: Array.from(transformedMap.entries()),
  set: Array.from(parsedSet.values()),
  setNonempty: z.set(z.string()).nonempty().safeParse(new Set()).success,
  setFail: z.set(z.string()).min(2).safeParse(new Set(["a"])).success,
  setMax: z.set(z.string()).max(2).safeParse(new Set(["a", "b", "c"])).success,
  setSize: z.set(z.string()).size(2).safeParse(new Set(["a", "b"])).success,
  transformedSet: Array.from(transformedSet.values()),
});

print("composition", {
  intersection: z.intersection(z.object({ a: z.string() }), z.object({ b: z.number() })).parse({ a: "x", b: 1 }),
  or: z.string().or(z.number()).parse(5),
  and: z.object({ a: z.string() }).and(z.object({ b: z.boolean() })).parse({ a: "x", b: true }),
  strictObject: z.strictObject({ id: z.number() }).safeParse({ id: 1, extra: true }).success,
  schemaArray: z.string().array().parse(["a", "b"]),
  schemaOr: z.literal("a").or(z.literal("b")).parse("b"),
  pipelineFactory: z.pipeline(z.string().transform((value) => value.length), z.number().min(2)).safeParse("abc").success,
});

const csvSchema = z
  .string()
  .transform((value) => value.split(",").map((item) => item.trim()).filter(Boolean))
  .refine((items) => items.length >= 2, { message: "need two values" });
print("transform", csvSchema.parse("a, b, c"));
print("refine", csvSchema.safeParse("single").success);

const preprocessSchema = z.preprocess((value) => (typeof value === "string" ? value.trim() : value), z.string().min(2));
const matchingPasswords = z.object({ password: z.string(), confirm: z.string() }).refine((values) => values.password === values.confirm, {
  path: ["confirm"],
  message: "password mismatch",
});
const superRefineSchema = z.array(z.number()).superRefine((values, ctx) => {
  if (new Set(values).size !== values.length) {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: "duplicates" });
  }
});
const matchingPasswordsResult = matchingPasswords.safeParse({ password: "a", confirm: "b" });
print("effects", {
  preprocess: preprocessSchema.parse(" ok "),
  pipe: z.string().transform((value) => value.length).pipe(z.number().min(2)).safeParse("abc").success,
  superRefine: superRefineSchema.safeParse([1, 1]).success,
  custom: z.custom<string>((value) => value === "token").safeParse("token").success,
  refinePath: matchingPasswordsResult.success ? "ok" : matchingPasswordsResult.error.issues[0].path.join("."),
  refineMessage: matchingPasswordsResult.success ? "ok" : matchingPasswordsResult.error.issues[0].message,
});

print("modifiers", {
  ostring: z.ostring().parse(undefined) === undefined,
  onumber: z.onumber().parse(undefined) === undefined,
  oboolean: z.oboolean().parse(undefined) === undefined,
  optional: z.string().optional().parse(undefined) === undefined,
  nullable: z.string().nullable().parse(null) === null,
  nullish: z.string().nullish().parse(undefined) === undefined,
  default: z.string().default("fallback").parse(undefined),
  catch: z.number().catch(9).parse("bad"),
  catchFunction: z.number().catch((ctx) => ctx.error.issues.length).parse("bad"),
  brand: z.string().brand<"FixtureId">().parse("id-1"),
  described: z.string().describe("fixture string").description,
  describedObject: z.object({ id: z.number() }).describe("fixture object").description,
  describedParse: z.string().describe("parse still works").parse("described"),
  optionalUnwrap: z.string().optional().unwrap().parse("wrapped"),
  nullableUnwrap: z.string().nullable().unwrap().parse("wrapped"),
  arrayElement: z.array(z.string()).element.parse("element"),
  promiseUnwrap: z.promise(z.number()).unwrap().parse(3),
  isOptional: z.string().optional().isOptional(),
  isNullable: z.string().nullable().isNullable(),
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
const invalidBoxResult = z.instanceof(Box, { message: "not a box" }).safeParse({});
const customDateSchema = z.date({
  required_error: "required date",
  invalid_type_error: "not a date",
});
const requiredDateResult = customDateSchema.safeParse(undefined);
const invalidDateResult = customDateSchema.safeParse("nope");
print("instances.dates", {
  instanceof: z.instanceof(Box).parse(new Box("ok")).value,
  instanceMessage: invalidBoxResult.success ? "ok" : invalidBoxResult.error.issues[0].message,
  date: z.date().parse(new Date("2020-01-02T00:00:00.000Z")).toISOString(),
  dateRequired: requiredDateResult.success ? "ok" : requiredDateResult.error.issues[0].message,
  dateInvalid: invalidDateResult.success ? "ok" : invalidDateResult.error.issues[0].message,
  dateMin: z.date().min(new Date("2020-01-01T00:00:00.000Z")).safeParse(new Date("2020-01-02T00:00:00.000Z")).success,
  dateMax: z.date().max(new Date("2020-01-03T00:00:00.000Z")).safeParse(new Date("2020-01-04T00:00:00.000Z")).success,
});

const validatedFn = z.function().args(z.string()).returns(z.number()).implement((value) => value.trim().length);
const invalidReturnFn = z.function().args(z.string()).returns(z.number()).implement(() => "bad" as unknown as number);
const functionSchema = z.function().args(z.string(), z.number()).returns(z.boolean());
const asyncValidatedFn = z.function().args(z.string()).returns(z.promise(z.number())).implement(async (value) => value.trim().length);
const asyncInvalidReturnFn = z.function().args(z.string()).returns(z.promise(z.number())).implement(async () => "bad" as unknown as number);
const invalidAsyncReturn = await (async () => {
  try {
    await asyncInvalidReturnFn("x");
    return false;
  } catch {
    return true;
  }
})();
print("function", {
  valid: validatedFn(" tuna "),
  asyncValid: await asyncValidatedFn(" salmon "),
  parameters: functionSchema.parameters().items.length,
  returnType: functionSchema.returnType().safeParse(true).success,
  invalidArgs: (() => {
    try {
      (validatedFn as unknown as (value: number) => number)(1);
      return false;
    } catch {
      return true;
    }
  })(),
  invalidReturns: (() => {
    try {
      invalidReturnFn("x");
      return false;
    } catch {
      return true;
    }
  })(),
  invalidAsyncReturn,
});

const asyncSchema = z.string().refine(async (value) => value === "ok");
const promisedNumber = await z.promise(z.number()).parse(Promise.resolve(5));
const methodPromisedNumber = await z.number().promise().parse(Promise.resolve(3));
const asyncParsed = await asyncSchema.parseAsync("ok");
const spaResult = await z.string().spa("ok");
const promisedFailure = await (async () => {
  try {
    await z.promise(z.number()).parse(Promise.resolve("bad"));
    return false;
  } catch {
    return true;
  }
})();
const rejectedReason = await (async () => {
  try {
    await z.promise(z.number()).parse(Promise.reject(new Error("promise boom")));
    return "ok";
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
})();
print("async", {
  refine: await asyncSchema.safeParseAsync("ok"),
  parseAsync: asyncParsed,
  spa: spaResult.success,
  promise: promisedNumber,
  methodPromise: methodPromisedNumber,
  promiseRejects: promisedFailure,
  rejectedReason,
});

const formatted = objectBase.safeParse({ id: "x", name: 1 });
if (!formatted.success) {
  const nestedFormatted = z.object({ user: z.object({ tags: z.array(z.string().min(2)).min(2) }) }).safeParse({ user: { tags: ["x"] } });
  print("errors", {
    formatKeys: Object.keys(formatted.error.format()).sort(),
    flatten: formatted.error.flatten(),
    nestedTagErrors: nestedFormatted.success ? [] : nestedFormatted.error.format().user?.tags?._errors,
    nestedTagItemErrors: nestedFormatted.success ? [] : nestedFormatted.error.format().user?.tags?.[0]?._errors,
  });
}

const customMessageString = z.string({
  required_error: "required string",
  invalid_type_error: "not a string",
});
const requiredStringResult = customMessageString.safeParse(undefined);
const invalidStringResult = customMessageString.safeParse(1);
const minMessageResult = z.string().min(3, "too short").safeParse("x");
const parsePathResult = z.string().safeParse(1, { path: ["root"] });
const perParseMapResult = z.string().safeParse(1, {
  errorMap: (issue) => ({ message: `local:${issue.code}` }),
});
print("customErrors", {
  required: requiredStringResult.success ? "ok" : requiredStringResult.error.issues[0].message,
  invalidType: invalidStringResult.success ? "ok" : invalidStringResult.error.issues[0].message,
  min: minMessageResult.success ? "ok" : minMessageResult.error.issues[0].message,
  path: parsePathResult.success ? "ok" : parsePathResult.error.issues[0].path.join("."),
  perParseMap: perParseMapResult.success ? "ok" : perParseMapResult.error.issues[0].message,
});

const summarySchema = z.array(userSchema.pick({ id: true, role: true })).min(1);
print("array", summarySchema.parse([{ id: 1, role: "admin" }]));

const standardOk = objectBase["~standard"].validate({ id: 1, name: "a" });
const standardBad = objectBase["~standard"].validate({ id: "x", name: 1 });
print("standard", {
  vendor: objectBase["~standard"].vendor,
  version: objectBase["~standard"].version,
  ok: "value" in standardOk ? standardOk.value : null,
  badIssues: "issues" in standardBad ? standardBad.issues?.length : 0,
});

const originalErrorMap = z.getErrorMap();
z.setErrorMap((issue) => ({ message: `mapped:${issue.code}` }));
const mappedError = z.string().safeParse(1);
z.setErrorMap(originalErrorMap);
print("errorMap", {
  mapped: mappedError.success ? "ok" : mappedError.error.issues[0].message,
});
