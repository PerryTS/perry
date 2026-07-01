import zDefault, { z } from "zod";
import * as zNamespace from "zod";
import z3Default, { z as z3 } from "zod/v3";
import * as z3Namespace from "zod/v3";

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

function issueMetadata(error: z.ZodError) {
  return error.issues.map((issue) => {
    const metadata: Record<string, unknown> = {
      code: issue.code,
      path: issue.path.join("."),
    };
    for (const key of [
      "expected",
      "received",
      "keys",
      "options",
      "validation",
      "minimum",
      "maximum",
      "inclusive",
      "exact",
      "multipleOf",
      "type",
      "params",
    ]) {
      if (key in issue) {
        metadata[key] = issue[key as keyof typeof issue];
      }
    }
    return metadata;
  });
}

function issueMetadataFromResult(result: z.SafeParseReturnType<unknown, unknown>) {
  return result.success ? [] : issueMetadata(result.error);
}

async function issueMetadataFromThrown(call: () => Promise<unknown>) {
  try {
    await call();
    return [];
  } catch (error) {
    return issueMetadata(error as z.ZodError);
  }
}

function thrownSummary(call: () => unknown) {
  try {
    call();
    return "ok";
  } catch (error) {
    return error instanceof Error ? `${error.constructor.name}:${error.message.split("\n")[0]}` : String(error);
  }
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

print("packageExports", {
  parsed: {
    string: z.getParsedType("x"),
    nan: z.getParsedType(NaN),
    map: z.getParsedType(new Map()),
    promise: z.getParsedType(Promise.resolve(1)),
  },
  status: {
    ok: z.OK(1),
    dirty: z.DIRTY(2),
    invalid: z.INVALID,
    isValid: z.isValid(z.OK(1)),
    isDirty: z.isDirty(z.DIRTY(2)),
    isAborted: z.isAborted(z.INVALID),
  },
  parsedType: {
    string: z.ZodParsedType.string,
    nan: z.ZodParsedType.nan,
    map: z.ZodParsedType.map,
    promise: z.ZodParsedType.promise,
  },
  typeKind: {
    object: z.ZodFirstPartyTypeKind.ZodObject,
    pipeline: z.ZodFirstPartyTypeKind.ZodPipeline,
    readonly: z.ZodFirstPartyTypeKind.ZodReadonly,
  },
  aliases: {
    schema: z.Schema === z.ZodType,
    schemaInstance: z.string() instanceof z.Schema,
    brand: typeof z.BRAND,
    brandDescription: z.BRAND.description,
    defaultEqualsNamed: zDefault === z,
    namespaceHasZ: zNamespace.z === z,
    namespaceDefaultEqualsNamed: zNamespace.default === z,
    namespaceString: zNamespace.string().parse("namespace"),
  },
  names: [z.ZodString.name, z.ZodNumber.name, z.ZodObject.name, z.ZodError.name],
});

const parseStatus = new z.ParseStatus();
parseStatus.dirty();
const parseStatusMerged = z.ParseStatus.mergeArray(parseStatus, [z.OK("a"), z.DIRTY("b")]);
parseStatus.abort();
const datetimeWithOffset = z.datetimeRegex({ offset: true, precision: 3 });
const validStatus = z.OK("valid");
const dirtyStatus = z.DIRTY("dirty");
const abortedStatus = z.INVALID;
print("packageUtilities", {
  isAsync: [z.isAsync(Promise.resolve(1)), z.isAsync(1)],
  parseStatus: {
    dirty: parseStatusMerged.status,
    merged: parseStatusMerged.value,
    aborted: parseStatus.value,
  },
  parsedTypes: [
    z.getParsedType(null),
    z.getParsedType([]),
    z.getParsedType(new Map()),
    z.getParsedType(1n),
    z.getParsedType(Promise.resolve(1)),
    z.getParsedType(NaN),
  ],
  statusHelpers: {
    valid: [validStatus.status, z.isValid(validStatus), z.isDirty(validStatus), z.isAborted(validStatus)],
    dirty: [dirtyStatus.status, z.isValid(dirtyStatus), z.isDirty(dirtyStatus), z.isAborted(dirtyStatus)],
    aborted: [abortedStatus.status, z.isValid(abortedStatus), z.isDirty(abortedStatus), z.isAborted(abortedStatus)],
    never: z.NEVER.status,
    emptyPathLength: z.EMPTY_PATH.length,
  },
  util: {
    arrayToEnum: z.util.arrayToEnum(["a", "b"]).b,
    validEnumValues: z.util.getValidEnumValues({ A: "a", B: 1, 1: "B" }),
    joinValues: z.util.joinValues(["a", 1, true]),
    objectKeys: z.util.objectKeys({ b: 1, a: 2 }).join("|"),
    objectValues: z.util.objectValues({ b: 1, a: 2 }),
    find: z.util.find([1, 2, 3], (value) => value > 1),
    isInteger: [z.util.isInteger(1), z.util.isInteger(1.5)],
    jsonStringifyReplacer: JSON.stringify({ n: 1n }, z.util.jsonStringifyReplacer),
  },
  quoteless: z.quotelessJson({ a: "x", nested: { b: 1 }, arr: [true, null] }).split("\n").length,
  datetimeRegex: [
    datetimeWithOffset.test("2020-01-02T03:04:05.123Z"),
    datetimeWithOffset.test("2020-01-02T03:04:05Z"),
  ],
  defaultError: z.defaultErrorMap(
    { code: z.ZodIssueCode.invalid_type, expected: "string", received: "number", path: [] },
    { data: 1, defaultError: "fallback" },
  ).message,
});

const directIssue = z.makeIssue({
  data: 1,
  path: ["root"],
  issueData: { code: z.ZodIssueCode.custom, path: ["leaf"], message: "direct" },
  errorMaps: [z.defaultErrorMap],
});
const mappedIssue = z.makeIssue({
  data: "bad",
  path: ["root"],
  issueData: { code: z.ZodIssueCode.invalid_type, expected: "number", received: "string" },
  errorMaps: [z.defaultErrorMap],
});
const parseContext = {
  common: { issues: [], async: false },
  path: ["ctx"],
  schemaErrorMap: undefined,
  parent: null,
  data: "bad",
  parsedType: z.ZodParsedType.string,
} as any;
z.addIssueToContext(parseContext, {
  code: z.ZodIssueCode.invalid_type,
  expected: "number",
  received: "string",
});
const transformerSchema = z.ZodTransformer.create(z.string(), {
  type: "transform",
  transform: (value: string) => value.length,
});
print("parseHelpers", {
  direct: { code: directIssue.code, path: directIssue.path.join("."), message: directIssue.message },
  mapped: { path: mappedIssue.path.join("."), message: mappedIssue.message },
  context: parseContext.common.issues.map((issue: z.ZodIssue) => ({
    path: issue.path.join("."),
    message: issue.message,
  })),
  aliases: {
    schema: z.string() instanceof z.ZodSchema,
    type: z.string() instanceof z.ZodType,
    transformer: transformerSchema instanceof z.ZodTransformer,
    transformerParse: transformerSchema.parse("tuna"),
  },
});

print("coerce", {
  string: z.coerce.string().parse(42),
  number: z.coerce.number().parse("12.5"),
  boolean: z.coerce.boolean().parse(1),
  bigint: z.coerce.bigint().parse("42").toString(),
  date: z.coerce.date().parse("2020-01-02T00:00:00.000Z").toISOString(),
});

print("primitiveEdges", {
  boolean: z.boolean().parse(true),
  booleanRejectsString: z.boolean().safeParse("true").success,
  coerceBooleanString: z.coerce.boolean().parse("false"),
  coerceNumberRejectsNaN: z.coerce.number().safeParse("not-a-number").success,
  coerceBigintRejectsText: z.coerce.bigint().safeParse("not-a-bigint").success,
  coerceDateEpoch: z.coerce.date().parse(0).toISOString(),
  coerceDateRejectsInvalid: z.coerce.date().safeParse("not-a-date").success,
});

const boundedDateSchema = z.date()
  .min(new Date("2020-01-01T00:00:00.000Z"), "too old")
  .max(new Date("2020-12-31T00:00:00.000Z"), "too new");
const primitiveInstanceSchema = z.instanceof(Date);
print("primitiveMetadata", {
  types: [
    z.string()._def.typeName,
    z.number()._def.typeName,
    z.bigint()._def.typeName,
    z.boolean()._def.typeName,
    z.nan()._def.typeName,
    z.symbol()._def.typeName,
    z.any()._def.typeName,
    z.unknown()._def.typeName,
    z.never()._def.typeName,
    z.undefined()._def.typeName,
    z.null()._def.typeName,
    z.void()._def.typeName,
  ],
  date: {
    typeName: boundedDateSchema._def.typeName,
    coerce: boundedDateSchema._def.coerce,
    minDate: boundedDateSchema.minDate?.toISOString(),
    maxDate: boundedDateSchema.maxDate?.toISOString(),
    checks: boundedDateSchema._def.checks.map((check) => ({
      kind: check.kind,
      value: check.value,
      message: check.message,
    })),
  },
  coerceDate: z.coerce.date()._def.coerce,
  instanceof: {
    typeName: primitiveInstanceSchema._def.typeName,
    inner: primitiveInstanceSchema._def.schema.constructor.name,
    effect: primitiveInstanceSchema._def.effect.type,
  },
});

const colorEnum = z.enum(["red", "blue", "green"]);
const literalReady = z.literal("ready");
const nativeTextEnum = z.nativeEnum({ A: "a", B: "b" } as const);
const nativeMixedEnumObject = { Text: "text", Zero: 0, One: 1, 0: "Zero", 1: "One" } as const;
const nativeMixedEnum = z.nativeEnum(nativeMixedEnumObject);
const nativeMixedReverseResult = nativeMixedEnum.safeParse("Zero");
const enumRequiredResult = z
  .enum(["red", "blue"], { required_error: "need color", invalid_type_error: "bad color" })
  .safeParse(undefined);
const enumInvalidTypeResult = z
  .enum(["red", "blue"], { required_error: "need color", invalid_type_error: "bad color" })
  .safeParse(1);
print("literals.enums", {
  literal: literalReady.safeParse("ready").success,
  literalBigint: z.literal(2n).safeParse(2n).success,
  literalBoolean: z.literal(true).safeParse(false).success,
  literalNull: z.literal(null).parse(null),
  literalFail: z.literal(3).safeParse(4).success,
  literalValue: literalReady.value,
  literalTypeName: literalReady._def.typeName,
  enum: colorEnum.parse("blue"),
  enumBlue: colorEnum.enum.blue,
  enumValues: colorEnum.Values.green,
  enumEnum: colorEnum.Enum.red,
  enumOptions: colorEnum.options.join("|"),
  enumTypeName: colorEnum._def.typeName,
  enumDefValues: colorEnum._def.values.join("|"),
  enumKeys: Object.keys(colorEnum.enum).sort(),
  enumExtract: colorEnum.extract(["red", "green"]).safeParse("blue").success,
  enumExtractOk: colorEnum.extract(["red", "green"]).parse("red"),
  enumExtractOptions: colorEnum.extract(["red", "green"]).options.join("|"),
  enumExclude: colorEnum.exclude(["blue"]).parse("green"),
  enumExcludeOptions: colorEnum.exclude(["blue"]).options.join("|"),
  enumRequiredMessage: enumRequiredResult.success ? "ok" : enumRequiredResult.error.issues[0].message,
  enumInvalidTypeMessage: enumInvalidTypeResult.success ? "ok" : enumInvalidTypeResult.error.issues[0].message,
  nativeEnum: nativeTextEnum.parse("a"),
  nativeEnumNumber: z.nativeEnum({ A: 1, B: 2 } as const).parse(2),
  nativeEnumMixed: [nativeMixedEnum.parse("text"), nativeMixedEnum.parse(0), nativeMixedEnum.parse(1)],
  nativeEnumReverseRejected: issueMetadataFromResult(nativeMixedReverseResult),
  nativeEnumTypeName: nativeTextEnum._def.typeName,
  nativeEnumKeys: Object.keys(nativeTextEnum.enum).join("|"),
  nativeEnumMixedKeys: Object.keys(nativeMixedEnum.enum).join("|"),
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
  startsWith: z.string().startsWith("tu").safeParse("tuna").success,
  startsWithFail: z.string().startsWith("tu").safeParse("fish").success,
  endsWith: z.string().endsWith("na").safeParse("tuna").success,
  endsWithFail: z.string().endsWith("na").safeParse("tune").success,
  trim: z.string().trim().parse("  tuna  "),
  uppercase: z.string().toUpperCase().parse("tuna"),
  lowercase: z.string().toLowerCase().parse("ABC"),
  chainedTransform: z.string().trim().toUpperCase().startsWith("TU").endsWith("NA").parse("  tuna  "),
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

const boundedString = z.string().min(2).max(5);
const formattedStrings = {
  email: z.string().email(),
  url: z.string().url(),
  uuid: z.string().uuid(),
  datetime: z.string().datetime(),
  ip: z.string().ip(),
};
const extraFormattedStrings = {
  base64: z.string().base64(),
  base64url: z.string().base64url(),
  cidr: z.string().cidr(),
  cuid: z.string().cuid(),
  cuid2: z.string().cuid2(),
  date: z.string().date(),
  duration: z.string().duration(),
  emoji: z.string().emoji(),
  nanoid: z.string().nanoid(),
  time: z.string().time(),
  ulid: z.string().ulid(),
};
const boundedNumber = z.number().int().min(1).max(3).finite();
print("schemaIntrospection", {
  stringBounds: [boundedString.minLength, boundedString.maxLength],
  stringFormats: Object.fromEntries(
    Object.entries(formattedStrings).map(([name, schema]) => [
      name,
      {
        isEmail: schema.isEmail,
        isURL: schema.isURL,
        isUUID: schema.isUUID,
        isDatetime: schema.isDatetime,
        isIP: schema.isIP,
      },
    ]),
  ),
  extraStringFormats: {
    base64: extraFormattedStrings.base64.isBase64,
    base64url: extraFormattedStrings.base64url.isBase64url,
    cidr: extraFormattedStrings.cidr.isCIDR,
    cuid: extraFormattedStrings.cuid.isCUID,
    cuid2: extraFormattedStrings.cuid2.isCUID2,
    date: extraFormattedStrings.date.isDate,
    duration: extraFormattedStrings.duration.isDuration,
    emoji: extraFormattedStrings.emoji.isEmoji,
    nanoid: extraFormattedStrings.nanoid.isNANOID,
    time: extraFormattedStrings.time.isTime,
    ulid: extraFormattedStrings.ulid.isULID,
  },
  numberBounds: [boundedNumber.minValue, boundedNumber.maxValue],
  numberFlags: { isInt: boundedNumber.isInt, isFinite: boundedNumber.isFinite },
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
  negative: z.bigint().negative().safeParse(-1n).success,
  nonnegative: z.bigint().nonnegative().safeParse(0n).success,
});

const eventSchema = z.discriminatedUnion("type", [
  z.object({ type: z.literal("text"), value: z.string() }),
  z.object({ type: z.literal("count"), value: z.number().int() }),
]);
const multiValueDiscriminatedSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.enum(["text", "markdown"]), body: z.string() }),
  z.object({ kind: z.literal("count"), value: z.number() }),
]);
const multiValueInvalidKind = multiValueDiscriminatedSchema.safeParse({ kind: "image", body: "x" });
const multiValueInvalidBranch = multiValueDiscriminatedSchema.safeParse({ kind: "text", body: 1 });
const inheritedDiscriminatedInput = Object.create({ type: "count", value: 3 });
inheritedDiscriminatedInput.extra = "own";
const inheritedDiscriminatedParsed = eventSchema.parse(inheritedDiscriminatedInput);
const inheritedDiscriminatedInvalidInput = Object.create({ type: "count", value: "bad" });
inheritedDiscriminatedInvalidInput.extra = "own";
const objectUnionSchema = z.union([
  z.object({ kind: z.literal("a"), value: z.string() }),
  z.object({ kind: z.literal("b"), count: z.number() }),
]);
const unionBranchFailure = objectUnionSchema.safeParse({ kind: "a", value: 1 });
const inheritedUnionInput = Object.create({ kind: "b", count: 2 });
inheritedUnionInput.extra = "own";
const inheritedUnionParsed = objectUnionSchema.parse(inheritedUnionInput);
print("discriminatedUnion", [
  eventSchema.parse({ type: "text", value: "hello" }),
  eventSchema.parse({ type: "count", value: 3 }),
]);
print("discriminatedUnion.options", {
  length: eventSchema.options.length,
  discriminator: eventSchema.discriminator,
  optionKeys: Array.from(eventSchema.optionsMap.keys()).join("|"),
  typeName: eventSchema._def.typeName,
  inherited: inheritedDiscriminatedParsed,
  inheritedOwnType: Object.prototype.hasOwnProperty.call(inheritedDiscriminatedParsed, "type"),
  inheritedOwnValue: Object.prototype.hasOwnProperty.call(inheritedDiscriminatedParsed, "value"),
  inheritedInvalid: issueMetadataFromResult(eventSchema.safeParse(inheritedDiscriminatedInvalidInput)),
  multiValue: [
    multiValueDiscriminatedSchema.parse({ kind: "text", body: "hello" }),
    multiValueDiscriminatedSchema.parse({ kind: "markdown", body: "**hello**" }),
  ],
  multiValueOptionKeys: Array.from(multiValueDiscriminatedSchema.optionsMap.keys()).join("|"),
  multiValueInvalidKind: issueMetadataFromResult(multiValueInvalidKind),
  multiValueInvalidBranch: issueMetadataFromResult(multiValueInvalidBranch),
});

const primitiveSchema = z.union([z.string().regex(/^id-/), z.number().int()]);
print("union", {
  results: [primitiveSchema.safeParse("id-42").success, primitiveSchema.safeParse(4.5).success],
  options: primitiveSchema.options.length,
  typeName: primitiveSchema._def.typeName,
  optionTypes: primitiveSchema._def.options.map((option) => option.constructor.name),
  branchFailure: unionBranchFailure.success
    ? []
    : unionBranchFailure.error.issues[0].unionErrors.map((error) =>
        error.issues.map((issue) => `${issue.path.join(".")}:${issue.code}`),
      ),
  inheritedObject: inheritedUnionParsed,
  inheritedObjectOwnKind: Object.prototype.hasOwnProperty.call(inheritedUnionParsed, "kind"),
  inheritedObjectOwnCount: Object.prototype.hasOwnProperty.call(inheritedUnionParsed, "count"),
});

const objectBase = z.object({ id: z.number(), name: z.string(), active: z.boolean().optional() });
const nestedObject = z.object({ nested: z.object({ label: z.string() }) });
const defaultedOptionalObject = z.object({ defaulted: z.string().default("tuna").optional() });
const maskedObjectBase = z.object({
  requiredName: z.string(),
  optionalAge: z.number().optional(),
  nested: z.object({ label: z.string() }),
});
const maskedPartialObject = maskedObjectBase.partial({ requiredName: true });
const maskedRequiredObject = maskedPartialObject.required({ requiredName: true });
const inheritedObjectInput = Object.create({ inherited: "from-proto" });
inheritedObjectInput.own = 1;
const inheritedObjectParsed = z.object({ own: z.number(), inherited: z.string() }).parse(inheritedObjectInput);
const inheritedPassthroughInput = Object.create({ inheritedExtra: "from-proto" });
inheritedPassthroughInput.id = 1;
const inheritedPassthroughParsed = z.object({ id: z.number() }).passthrough().parse(inheritedPassthroughInput);
const nullProtoInput = Object.create(null) as Record<string, unknown>;
nullProtoInput.id = 1;
nullProtoInput.extra = "x";
const nullProtoPassthroughParsed = z.object({ id: z.number() }).passthrough().parse(nullProtoInput);
const nullProtoStrippedParsed = z.object({ id: z.number() }).parse(nullProtoInput);
let getterOwnReads = 0;
let getterInheritedReads = 0;
const getterInput = Object.create({
  get inherited() {
    getterInheritedReads += 1;
    return "from-getter";
  },
});
Object.defineProperty(getterInput, "id", {
  enumerable: true,
  get() {
    getterOwnReads += 1;
    return 7;
  },
});
const getterObjectParsed = z.object({ id: z.number(), inherited: z.string() }).parse(getterInput);
const getterPassthroughParsed = z.object({ id: z.number() }).passthrough().parse(getterInput);
const partialObjectSchema = objectBase.partial();
const partialNameObjectSchema = objectBase.partial({ name: true });
const requiredObjectSchema = objectBase.required();
const requiredActiveObjectSchema = objectBase.required({ active: true });
const deepPartialObjectSchema = nestedObject.deepPartial();
const objectShapeTypes = (schema: z.AnyZodObject) =>
  Object.fromEntries(Object.entries(schema.shape).map(([key, value]) => [key, value.constructor.name]));
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
const recursiveEventSchema: z.ZodType<any> = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("leaf"), value: z.string() }),
  z.object({
    kind: z.literal("branch"),
    get children() {
      return z.array(recursiveEventSchema);
    },
  }),
]);
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
  spreadShape: z.object({ ...objectBase.shape, role: z.string() }).parse({
    id: 1,
    name: "a",
    role: "user",
  }),
  inheritedInput: inheritedObjectInput,
  inheritedOutput: inheritedObjectParsed,
  inheritedOwnOutput: Object.prototype.hasOwnProperty.call(inheritedObjectParsed, "inherited"),
  inheritedOwnInput: Object.prototype.hasOwnProperty.call(inheritedObjectInput, "inherited"),
  inheritedOutputPrototype: Object.getPrototypeOf(inheritedObjectParsed) === Object.prototype,
  inheritedPassthrough: inheritedPassthroughParsed,
  inheritedPassthroughOwn: Object.prototype.hasOwnProperty.call(inheritedPassthroughParsed, "inheritedExtra"),
  inheritedStrict: z.object({ id: z.number() }).strict().safeParse(inheritedPassthroughInput).success,
  nullProto: nullProtoPassthroughParsed,
  nullProtoStripped: nullProtoStrippedParsed,
  nullProtoInputPrototype: Object.getPrototypeOf(nullProtoInput) === null,
  nullProtoOutputPrototype: Object.getPrototypeOf(nullProtoPassthroughParsed) === Object.prototype,
  nullProtoStrict: z.object({ id: z.number() }).strict().safeParse(nullProtoInput).success,
  getterInput: getterObjectParsed,
  getterPassthrough: getterPassthroughParsed,
  getterReads: [getterOwnReads, getterInheritedReads],
  getterOutputOwnInherited: Object.prototype.hasOwnProperty.call(getterObjectParsed, "inherited"),
  partial: partialObjectSchema.parse({ id: 1 }),
  partialName: partialNameObjectSchema.safeParse({ id: 1 }).success,
  deepPartial: deepPartialObjectSchema.parse({ nested: {} }),
  required: requiredObjectSchema.safeParse({ id: 1, name: "a" }).success,
  requiredActive: requiredActiveObjectSchema.safeParse({ id: 1, name: "a" }).success,
  nestedRequired: nestedObject.required().safeParse({ nested: { label: "x" } }).success,
  partialRequiredMask: {
    baseMissingName: maskedObjectBase.safeParse({ nested: { label: "x" } }).success,
    partialMissingName: maskedPartialObject.safeParse({ nested: { label: "x" } }).success,
    partialMissingNested: maskedPartialObject.safeParse({}).success,
    requiredMissingName: maskedRequiredObject.safeParse({ nested: { label: "x" } }).success,
    requiredMissingOptionalAge: maskedRequiredObject.safeParse({ requiredName: "x", nested: { label: "x" } }).success,
    base: objectShapeTypes(maskedObjectBase),
    partial: objectShapeTypes(maskedPartialObject),
    required: objectShapeTypes(maskedRequiredObject),
    partialNameInner: maskedPartialObject.shape.requiredName._def.innerType.constructor.name,
  },
  defaultedOptionalEmpty: defaultedOptionalObject.parse({}),
  defaultedOptionalUndefined: defaultedOptionalObject.parse({ defaulted: undefined }),
  anyUnknownMissing: z.object({ a: z.any(), b: z.unknown() }).safeParse({}).success,
  unknownKeys: [
    objectBase._def.unknownKeys,
    objectBase.passthrough()._def.unknownKeys,
    objectBase.strict()._def.unknownKeys,
    objectBase.strip()._def.unknownKeys,
  ],
  catchallType: z.object({ id: z.number() }).catchall(z.string())._def.catchall.constructor.name,
  strictCreate: z.ZodObject.strictCreate({ id: z.number() }).safeParse({ id: 1, extra: true }).success,
  strictCreateParse: z.ZodObject.strictCreate({ id: z.number() }).parse({ id: 1 }).id,
  lazycreate: z.ZodObject.lazycreate(() => ({ name: z.string() })).parse({ name: "lazy" }).name,
  shapeMetadata: {
    partial: objectShapeTypes(partialObjectSchema),
    partialName: objectShapeTypes(partialNameObjectSchema),
    partialActiveInner: partialObjectSchema.shape.active._def.innerType.constructor.name,
    required: objectShapeTypes(requiredObjectSchema),
    requiredActive: objectShapeTypes(requiredActiveObjectSchema),
    deepPartial: [
      deepPartialObjectSchema.shape.nested.constructor.name,
      deepPartialObjectSchema.shape.nested._def.innerType.shape.label.constructor.name,
    ],
  },
});

const mergeLeftPassthrough = z.object({ a: z.string() }).passthrough();
const mergeRightStrict = z.object({ b: z.number() }).strict();
const mergeRightPassthrough = z.object({ b: z.number() }).passthrough();
const mergeLeftCatchall = z.object({ a: z.string() }).catchall(z.string());
const mergeRightCatchall = z.object({ b: z.number() }).catchall(z.number());
const mergedStrict = mergeLeftPassthrough.merge(mergeRightStrict);
const mergedPassthrough = mergeRightStrict.merge(mergeLeftPassthrough);
const mergedCatchall = mergeLeftCatchall.merge(mergeRightCatchall);
print("objectMergeSemantics", {
  extendOverwrite: z.object({ id: z.string() }).extend({ id: z.number() }).parse({ id: 1 }).id,
  strictWinsUnknownKeys: mergedStrict._def.unknownKeys,
  strictRejectsExtra: mergedStrict.safeParse({ a: "x", b: 1, extra: true }).success,
  passthroughWinsUnknownKeys: mergedPassthrough._def.unknownKeys,
  passthroughAllowsExtra: mergedPassthrough.safeParse({ a: "x", b: 1, extra: true }).success,
  catchallWinsType: mergedCatchall._def.catchall.constructor.name,
  catchallAcceptsNumber: mergedCatchall.safeParse({ a: "x", b: 1, extra: 2 }).success,
  catchallRejectsString: mergedCatchall.safeParse({ a: "x", b: 1, extra: "x" }).success,
  mergedShapeKeys: Object.keys(mergedCatchall.shape).join("|"),
});

print("recursiveObjects", {
  category: categorySchema.parse({ name: "root", subcategories: [{ name: "leaf", subcategories: [] }] }),
  categoryIssue: issueMetadataFromResult(categorySchema.safeParse({ name: "root", subcategories: [{ name: 1 }] })),
  mutual: postSchema.parse({ title: "post", author: { email: "a@example.com", posts: [] } }).author.email,
  mutualNested: postSchema.parse({
    title: "post",
    author: {
      email: "a@example.com",
      posts: [{ title: "nested", author: { email: "b@example.com", posts: [] } }],
    },
  }).author.posts[0].author.email,
  discriminatedRecursive: recursiveEventSchema.parse({ kind: "branch", children: [{ kind: "leaf", value: "x" }] }),
  discriminatedRecursiveIssue: issueMetadataFromResult(
    recursiveEventSchema.safeParse({ kind: "branch", children: [{ kind: "leaf", value: 1 }] }),
  ),
});

const arrayElementIssue = z.array(z.string()).safeParse([1]);
const tupleLengthIssue = z.tuple([z.string()]).safeParse(["a", "b"]);
const boundedArray = z.array(z.string()).min(1).max(3).length(2);
const tupleWithRest = z.tuple([z.string(), z.number()]).rest(z.boolean());
const arrayMinMessage = z.array(z.string()).min(2, "need two").safeParse(["a"]);
const arrayMaxMessage = z.array(z.string()).max(1, "too many").safeParse(["a", "b"]);
const arrayLengthMessage = z.array(z.string()).length(2, "exactly two").safeParse(["a"]);
const sparseArrayInput = ["a", , "c"] as (string | undefined)[];
const sparseOptionalArrayParsed = z.array(z.string().optional()).parse(sparseArrayInput);
const sparseRequiredArrayResult = z.array(z.string()).safeParse(sparseArrayInput);
const inheritedArrayInput = ["a", , "c"] as string[];
let inheritedArrayParsed: string[] = [];
let inheritedTupleParsed: [string, string, string] | [] = [];
Object.defineProperty(Array.prototype, "1", {
  configurable: true,
  enumerable: true,
  writable: true,
  value: "proto",
});
try {
  inheritedArrayParsed = z.array(z.string()).parse(inheritedArrayInput);
  inheritedTupleParsed = z.tuple([z.string(), z.string(), z.string()]).parse(inheritedArrayInput);
} finally {
  delete (Array.prototype as unknown as Record<string, unknown>)["1"];
}
print("arrays.tuples", {
  array: z.array(z.number()).min(2).max(3).parse([1, 2]),
  transformedArray: z.array(z.string().transform((value) => value.length)).parse(["aa", "bbb"]),
  exactLength: z.array(z.string()).length(2).safeParse(["a", "b"]).success,
  nonempty: z.array(z.string()).nonempty().safeParse([]).success,
  elementDescription: z.array(z.string().describe("array item")).element.description,
  elementIssuePath: arrayElementIssue.success ? "ok" : arrayElementIssue.error.issues[0].path.join("."),
  typeName: boundedArray._def.typeName,
  bounds: [
    boundedArray._def.minLength?.value,
    boundedArray._def.maxLength?.value,
    boundedArray._def.exactLength?.value,
  ],
  elementType: boundedArray.element.constructor.name,
  minMessage: arrayMinMessage.success ? "ok" : arrayMinMessage.error.issues[0].message,
  maxMessage: arrayMaxMessage.success ? "ok" : arrayMaxMessage.error.issues[0].message,
  lengthMessage: arrayLengthMessage.success ? "ok" : arrayLengthMessage.error.issues[0].message,
  sparseOptional: sparseOptionalArrayParsed,
  sparseOptionalOwn1: Object.prototype.hasOwnProperty.call(sparseOptionalArrayParsed, "1"),
  sparseRequiredIssue: issueMetadataFromResult(sparseRequiredArrayResult),
  inheritedIndexArray: inheritedArrayParsed,
  inheritedIndexArrayOwn1: Object.prototype.hasOwnProperty.call(inheritedArrayParsed, "1"),
  inheritedIndexTuple: inheritedTupleParsed,
  inheritedIndexTupleOwn1: Object.prototype.hasOwnProperty.call(inheritedTupleParsed, "1"),
  tuple: z.tuple([z.string(), z.number()]).parse(["a", 1]),
  tupleLengthIssue: tupleLengthIssue.success ? "ok" : tupleLengthIssue.error.issues[0].code,
  tupleRest: z.tuple([z.string()]).rest(z.number()).parse(["a", 1, 2]),
  tupleItems: tupleWithRest.items.map((item) => item.constructor.name),
  tupleRestType: tupleWithRest._def.rest?.constructor.name,
  tupleTypeName: tupleWithRest._def.typeName,
});

const parsedMap = z.map(z.string(), z.number()).parse(new Map([["a", 1], ["b", 2]]));
const parsedSet = z.set(z.string()).min(2).parse(new Set(["a", "b"]));
const transformedMap = z
  .map(z.string(), z.string().transform((value) => value.length))
  .parse(new Map([["size", "abcd"]]));
const transformedSet = z
  .set(z.string().transform((value) => value.length))
  .parse(new Set(["aa", "bbb"]));
const cloneInput = { nested: { label: "x" }, tags: ["a"] };
const cloneParsed = z.object({ nested: z.object({ label: z.string() }), tags: z.array(z.string()) }).parse(cloneInput);
cloneParsed.nested.label = "changed";
cloneParsed.tags.push("b");
class FixtureMap extends Map<string, { count: number }> {}
class FixtureSet extends Set<{ id: number }> {}
const cloneMapInput = new Map([["a", { count: 1 }]]);
const cloneMapParsed = z.map(z.string(), z.object({ count: z.number() })).parse(cloneMapInput);
cloneMapParsed.get("a")!.count = 2;
const subclassMapInput = new FixtureMap([["a", { count: 1 }]]);
const subclassMapParsed = z.map(z.string(), z.object({ count: z.number() })).parse(subclassMapInput);
const subclassMapParsedValue = subclassMapParsed.get("a")!;
subclassMapParsedValue.count = 2;
const cloneMapKeyValueKey = { id: 1 };
const cloneMapKeyValueValue = { count: 1 };
const cloneMapKeyValueInput = new Map([[cloneMapKeyValueKey, cloneMapKeyValueValue]]);
const cloneMapKeyValueParsed = z
  .map(z.object({ id: z.number() }), z.object({ count: z.number() }))
  .parse(cloneMapKeyValueInput);
const cloneMapKeyValueParsedKey = Array.from(cloneMapKeyValueParsed.keys())[0];
const cloneMapKeyValueParsedValue = cloneMapKeyValueParsed.get(cloneMapKeyValueParsedKey)!;
cloneMapKeyValueParsedKey.id = 2;
cloneMapKeyValueParsedValue.count = 2;
const cloneSetValue = { id: 1 };
const cloneSetInput = new Set([cloneSetValue]);
const cloneSetParsed = z.set(z.object({ id: z.number() })).parse(cloneSetInput);
const cloneSetParsedValue = Array.from(cloneSetParsed)[0];
cloneSetParsedValue.id = 2;
const subclassSetValue = { id: 1 };
const subclassSetInput = new FixtureSet([subclassSetValue]);
const subclassSetParsed = z.set(z.object({ id: z.number() })).parse(subclassSetInput);
const subclassSetParsedValue = Array.from(subclassSetParsed)[0];
subclassSetParsedValue.id = 2;
const cloneDateInput = new Date("2020-01-02T00:00:00.000Z");
const cloneDateParsed = z.date().parse(cloneDateInput);
cloneDateParsed.setUTCFullYear(2021);
const inheritedRecordInput = Object.create({ inherited: 2 });
inheritedRecordInput.own = 1;
const inheritedRecordParsed = z.record(z.number()).parse(inheritedRecordInput);
const inheritedRecordInvalidInput = Object.create({ inherited: 2, badInherited: "x" });
inheritedRecordInvalidInput.own = 1;
const recordHiddenSymbol = Symbol("record-hidden");
const recordHiddenInput: Record<string | symbol, unknown> = { own: 1 };
Object.defineProperty(recordHiddenInput, "hidden", { value: 2, enumerable: false });
recordHiddenInput[recordHiddenSymbol] = 3;
const recordHiddenParsed = z.record(z.number()).parse(recordHiddenInput);
const recordMetadataSchema = z.record(z.string(), z.number());
const mapMetadataSchema = z.map(z.string(), z.number());
const setMinMetadataSchema = z.set(z.string()).min(1);
const setMaxMetadataSchema = z.set(z.string()).max(3);
const setSizeMetadataSchema = z.set(z.string()).size(2);
const setMinMessage = z.set(z.string()).min(2, "need two").safeParse(new Set(["a"]));
const setMaxMessage = z.set(z.string()).max(1, "too many").safeParse(new Set(["a", "b"]));
const setSizeMessage = z.set(z.string()).size(2, "exactly two").safeParse(new Set(["a"]));
const collectionIssueSummary = (result: z.SafeParseReturnType<unknown, unknown>) =>
  result.success ? [] : result.error.issues.map((issue) => ({ code: issue.code, path: issue.path.join(".") }));
print("collections", {
  record: z.record(z.number()).safeParse({ a: 1, b: 2 }).success,
  keyedRecord: z.record(z.enum(["a", "b"]), z.number()).safeParse({ a: 1, b: 2 }).success,
  keyedRecordMissing: z.record(z.enum(["a", "b"]), z.number()).safeParse({ a: 1 }).success,
  stringKeyRecord: z.record(z.string().min(1), z.number()).safeParse({ "": 1 }).success,
  transformedRecord: z
    .record(z.string().transform((value) => value.length))
    .parse({ a: "abcd" }),
  recordFail: z.record(z.number()).refine((scores) => Object.values(scores).every((score) => score >= 0)).safeParse({ a: 1, b: -1 }).success,
  recordNumberKey: collectionIssueSummary(z.record(z.number(), z.string()).safeParse({ 1: "one" })),
  recordNumericStringKey: z.record(z.string().regex(/^\d+$/), z.string()).parse({ 1: "one" }),
  recordNumericStringFail: collectionIssueSummary(z.record(z.string().regex(/^\d+$/), z.string()).safeParse({ abc: "bad" })),
  inheritedRecord: inheritedRecordParsed,
  inheritedRecordOwn: Object.prototype.hasOwnProperty.call(inheritedRecordParsed, "inherited"),
  inheritedRecordIssue: collectionIssueSummary(z.record(z.number()).safeParse(inheritedRecordInvalidInput)),
  nullProtoRecord: z.record(z.union([z.number(), z.string()])).parse(nullProtoInput),
  hiddenSymbolRecord: recordHiddenParsed,
  hiddenSymbolRecordHasHidden: Object.prototype.hasOwnProperty.call(recordHiddenParsed, "hidden"),
  hiddenSymbolRecordSymbols: Object.getOwnPropertySymbols(recordHiddenParsed).length,
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
  recordMeta: {
    typeName: recordMetadataSchema._def.typeName,
    key: recordMetadataSchema.keySchema.constructor.name,
    value: recordMetadataSchema.valueSchema.constructor.name,
    element: recordMetadataSchema.element.constructor.name,
  },
  mapMeta: {
    typeName: mapMetadataSchema._def.typeName,
    key: mapMetadataSchema.keySchema.constructor.name,
    value: mapMetadataSchema.valueSchema.constructor.name,
  },
  setMeta: {
    typeName: setSizeMetadataSchema._def.typeName,
    value: setSizeMetadataSchema._def.valueType.constructor.name,
    min: setMinMetadataSchema._def.minSize?.value,
    max: setMaxMetadataSchema._def.maxSize?.value,
    size: [
      setSizeMetadataSchema._def.minSize?.value,
      setSizeMetadataSchema._def.maxSize?.value,
    ],
  },
  setMessages: [
    setMinMessage.success ? "ok" : setMinMessage.error.issues[0].message,
    setMaxMessage.success ? "ok" : setMaxMessage.error.issues[0].message,
    setSizeMessage.success ? "ok" : setSizeMessage.error.issues[0].message,
  ],
  recordIssues: collectionIssueSummary(z.record(z.number()).safeParse({ ok: 1, bad: "x" })),
  mapKeyIssues: collectionIssueSummary(z.map(z.string().min(2), z.number()).safeParse(new Map([["a", 1]]))),
  mapValueIssues: collectionIssueSummary(z.map(z.string(), z.number()).safeParse(new Map([["bad", "x"]]))),
  setElementIssues: collectionIssueSummary(z.set(z.string().min(2)).safeParse(new Set(["a"]))),
  setSizeIssues: collectionIssueSummary(z.set(z.string()).size(2).safeParse(new Set(["a"]))),
});

print("cloneSemantics", {
  objectIdentity: cloneParsed !== cloneInput,
  nestedIdentity: cloneParsed.nested !== cloneInput.nested,
  arrayIdentity: cloneParsed.tags !== cloneInput.tags,
  objectSource: cloneInput,
  mapIdentity: cloneMapParsed !== cloneMapInput,
  mapValueIdentity: cloneMapParsed.get("a") !== cloneMapInput.get("a"),
  mapSource: Array.from(cloneMapInput.entries()),
  subclassMapIsMap: subclassMapParsed instanceof Map,
  subclassMapIsSubclass: subclassMapParsed instanceof FixtureMap,
  subclassMapValueIdentity: subclassMapParsedValue !== subclassMapInput.get("a"),
  subclassMapSource: Array.from(subclassMapInput.entries()),
  subclassMapParsed: Array.from(subclassMapParsed.entries()),
  mapObjectKeyIdentity: cloneMapKeyValueParsedKey !== cloneMapKeyValueKey,
  mapObjectValueIdentity: cloneMapKeyValueParsedValue !== cloneMapKeyValueValue,
  mapObjectSource: Array.from(cloneMapKeyValueInput.entries()),
  mapObjectParsed: Array.from(cloneMapKeyValueParsed.entries()),
  setIdentity: cloneSetParsed !== cloneSetInput,
  setValueIdentity: cloneSetParsedValue !== cloneSetValue,
  setSource: Array.from(cloneSetInput.values()),
  subclassSetIsSet: subclassSetParsed instanceof Set,
  subclassSetIsSubclass: subclassSetParsed instanceof FixtureSet,
  subclassSetValueIdentity: subclassSetParsedValue !== subclassSetValue,
  subclassSetSource: Array.from(subclassSetInput.values()),
  subclassSetParsed: Array.from(subclassSetParsed.values()),
  dateIdentity: cloneDateParsed !== cloneDateInput,
  dateSource: cloneDateInput,
  dateParsed: cloneDateParsed,
});

const intersectionObjectSchema = z.intersection(z.object({ a: z.string() }), z.object({ b: z.number() }));
const inheritedIntersectionInput = Object.create({ b: 2 });
inheritedIntersectionInput.a = "x";
const inheritedIntersectionParsed = intersectionObjectSchema.parse(inheritedIntersectionInput);
const nestedIntersectionSchema = z.intersection(
  z.object({ nested: z.object({ a: z.string() }), shared: z.literal("same") }),
  z.object({ nested: z.object({ b: z.number() }), shared: z.literal("same") }),
);
const transformedIntersectionSchema = z.intersection(
  z.string().transform(() => ({ a: 1 })),
  z.string().transform(() => ({ b: 2 })),
);
const conflictingIntersectionSchema = z.intersection(
  z.string().transform(() => ({ shared: { value: 1 } })),
  z.string().transform(() => ({ shared: { value: 2 } })),
);
const arrayIntersectionConflictSchema = z.intersection(
  z.string().transform(() => [1, 2]),
  z.string().transform(() => [1, 3]),
);
print("composition", {
  intersection: intersectionObjectSchema.parse({ a: "x", b: 1 }),
  inheritedIntersection: inheritedIntersectionParsed,
  inheritedIntersectionOwnB: Object.prototype.hasOwnProperty.call(inheritedIntersectionParsed, "b"),
  nestedIntersection: nestedIntersectionSchema.parse({ nested: { a: "x", b: 1 }, shared: "same" }),
  conflictingIntersection: issueMetadataFromResult(conflictingIntersectionSchema.safeParse("x")),
  arrayIntersectionConflict: issueMetadataFromResult(arrayIntersectionConflictSchema.safeParse("x")),
  intersectionTypeName: intersectionObjectSchema._def.typeName,
  intersectionSides: [
    intersectionObjectSchema._def.left.constructor.name,
    intersectionObjectSchema._def.right.constructor.name,
  ],
  transformedIntersection: transformedIntersectionSchema.parse("x"),
  or: z.string().or(z.number()).parse(5),
  and: z.object({ a: z.string() }).and(z.object({ b: z.boolean() })).parse({ a: "x", b: true }),
  strictObject: z.strictObject({ id: z.number() }).safeParse({ id: 1, extra: true }).success,
  schemaArray: z.string().array().parse(["a", "b"]),
  schemaOr: z.literal("a").or(z.literal("b")).parse("b"),
  schemaOrIssue: issueMetadataFromResult(z.string().min(2).or(z.number().min(10)).safeParse(5)),
  schemaAnd: z.object({ a: z.string() }).and(z.object({ b: z.number() })).parse({ a: "x", b: 1 }),
  pipelineFactory: z.pipeline(z.string().transform((value) => value.length), z.number().min(2)).safeParse("abc").success,
});

const csvSchema = z
  .string()
  .transform((value) => value.split(",").map((item) => item.trim()).filter(Boolean))
  .refine((items) => items.length >= 2, { message: "need two values" });
print("transform", csvSchema.parse("a, b, c"));
print("refine", csvSchema.safeParse("single").success);

const preprocessSchema = z.preprocess((value) => (typeof value === "string" ? value.trim() : value), z.string().min(2));
const metadataPreprocessSchema = z.preprocess((value) => value, z.string());
const metadataTransformSchema = z.string().transform((value) => value.length);
const metadataRefineSchema = z.string().refine((value) => value.length > 1, "too short");
const metadataPipelineSchema = z.string().transform((value) => value.length).pipe(z.number().min(1));
const metadataPipelineFactorySchema = z.pipeline(z.string(), z.coerce.number());
const matchingPasswords = z.object({ password: z.string(), confirm: z.string() }).refine((values) => values.password === values.confirm, {
  path: ["confirm"],
  message: "password mismatch",
});
const superRefineSchema = z.array(z.number()).superRefine((values, ctx) => {
  if (new Set(values).size !== values.length) {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: "duplicates" });
  }
});
const fatalCalls: string[] = [];
const fatalRefineSchema = z
  .string()
  .superRefine((value, ctx) => {
    fatalCalls.push(`first:${value}`);
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: "stop", fatal: true });
    return z.NEVER;
  })
  .superRefine(() => {
    fatalCalls.push("second");
  });
const neverTransformSchema = z
  .string()
  .transform((value, ctx) => {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: `bad transform:${value}` });
    return z.NEVER;
  })
  .pipe(z.string());
const matchingPasswordsResult = matchingPasswords.safeParse({ password: "a", confirm: "b" });
const fatalRefineResult = fatalRefineSchema.safeParse("x");
const neverTransformResult = neverTransformSchema.safeParse("x");
const refinementAliasResult = z
  .string()
  .refinement((value) => value === "ok", { code: z.ZodIssueCode.custom, message: "bad refinement" })
  .safeParse("no");
print("effects", {
  preprocess: preprocessSchema.parse(" ok "),
  pipe: z.string().transform((value) => value.length).pipe(z.number().min(2)).safeParse("abc").success,
  superRefine: superRefineSchema.safeParse([1, 1]).success,
  custom: z.custom<string>((value) => value === "token").safeParse("token").success,
  refinementAlias: refinementAliasResult.success ? "ok" : refinementAliasResult.error.issues[0].message,
  innerType: z.string().transform((value) => value.length).innerType().parse("inner"),
  sourceType: z.string().transform((value) => value.length).sourceType().parse("source"),
  refinePath: matchingPasswordsResult.success ? "ok" : matchingPasswordsResult.error.issues[0].path.join("."),
  refineMessage: matchingPasswordsResult.success ? "ok" : matchingPasswordsResult.error.issues[0].message,
  fatalCalls,
  fatalIssue: fatalRefineResult.success ? "ok" : fatalRefineResult.error.issues[0].message,
  fatalFlag: fatalRefineResult.success ? false : fatalRefineResult.error.issues[0].fatal === true,
  neverTransform: neverTransformResult.success ? "ok" : neverTransformResult.error.issues[0].message,
  neverStatus: z.NEVER.status,
  metadata: {
    preprocess: [
      metadataPreprocessSchema._def.typeName,
      metadataPreprocessSchema._def.effect.type,
      metadataPreprocessSchema._def.schema.constructor.name,
      metadataPreprocessSchema.innerType().constructor.name,
      metadataPreprocessSchema.sourceType().constructor.name,
    ],
    transform: [
      metadataTransformSchema._def.typeName,
      metadataTransformSchema._def.effect.type,
      metadataTransformSchema._def.schema.constructor.name,
      metadataTransformSchema.innerType().constructor.name,
      metadataTransformSchema.sourceType().constructor.name,
    ],
    refine: [
      metadataRefineSchema._def.typeName,
      metadataRefineSchema._def.effect.type,
      metadataRefineSchema._def.schema.constructor.name,
    ],
    pipeline: [
      metadataPipelineSchema._def.typeName,
      metadataPipelineSchema._def.in.constructor.name,
      metadataPipelineSchema._def.out.constructor.name,
      metadataPipelineFactorySchema._def.in.constructor.name,
      metadataPipelineFactorySchema._def.out.constructor.name,
    ],
  },
});

const customTokenResult = z.custom<string>((value) => typeof value === "string" && value.startsWith("tok_")).safeParse("tok_123");
const customTokenFailure = z.custom<string>((value) => typeof value === "string" && value.startsWith("tok_"), {
  message: "not a token",
}).safeParse("bad");
print("customSchemas", {
  unchecked: z.custom<{ id: number }>().parse({ id: 1 }).id,
  predicate: customTokenResult.success,
  message: customTokenFailure.success ? "ok" : customTokenFailure.error.issues[0].message,
});

const originalDescriptionSchema = z.string();
const describedStringSchema = originalDescriptionSchema.describe("fixture string");
const describedObjectSchema = z.object({ id: z.number().describe("identifier") }).describe("fixture object");
const describedArraySchema = z.array(z.string().describe("array item")).describe("fixture array");
const describedOptionalSchema = z.string().describe("inner").optional().describe("outer optional");
print("descriptions", {
  original: originalDescriptionSchema.description ?? null,
  string: [describedStringSchema.description, describedStringSchema._def.description, describedStringSchema.parse("ok")],
  object: [
    describedObjectSchema.description,
    describedObjectSchema._def.description,
    describedObjectSchema.shape.id.description,
    describedObjectSchema.parse({ id: 1 }).id,
  ],
  array: [
    describedArraySchema.description,
    describedArraySchema._def.description,
    describedArraySchema.element.description,
    describedArraySchema.parse(["x"]).length,
  ],
  optional: [
    describedOptionalSchema.description,
    describedOptionalSchema._def.description,
    describedOptionalSchema._def.innerType.description,
    describedOptionalSchema.parse(undefined) === undefined,
  ],
});

const optionalMetadataSchema = z.string().optional();
const nullableMetadataSchema = z.string().nullable();
const defaultMetadataSchema = z.string().default("fallback");
const catchMetadataSchema = z.number().catch(9);
const promiseMetadataSchema = z.promise(z.number());
const readonlyMetadataSchema = z.object({ id: z.number() }).readonly();
const brandedMetadataSchema = z.string().brand<"FixtureId">();
const readonlyObjectValue = z.object({ id: z.number() }).readonly().parse({ id: 1 });
const readonlyNestedInput = { nested: { id: 1 }, tags: ["a"] };
const readonlyNestedValue = z
  .object({ nested: z.object({ id: z.number() }), tags: z.array(z.string()) })
  .readonly()
  .parse(readonlyNestedInput);
const readonlyArrayValue = z.array(z.string()).readonly().parse(["a"]);
const readonlyTupleValue = z.tuple([z.string()]).readonly().parse(["a"]);
const readonlyMapValue = z.map(z.string(), z.number()).readonly().parse(new Map([["a", 1]]));
const readonlySetValue = z.set(z.string()).readonly().parse(new Set(["a"]));
let defaultFactoryCalls = 0;
let catchFactoryCalls = 0;
const countedDefaultSchema = z.string().default(() => `fallback-${++defaultFactoryCalls}`);
const countedCatchSchema = z.number().catch((ctx) => {
  catchFactoryCalls += 1;
  return ctx.error.issues.length + catchFactoryCalls;
});
const tryErrorName = (fn: () => void) => {
  try {
    fn();
    return "ok";
  } catch (error) {
    return error instanceof Error ? error.constructor.name : String(error);
  }
};
print("modifiers", {
  ostring: z.ostring().parse(undefined) === undefined,
  onumber: z.onumber().parse(undefined) === undefined,
  oboolean: z.oboolean().parse(undefined) === undefined,
  optional: z.string().optional().parse(undefined) === undefined,
  nullable: z.string().nullable().parse(null) === null,
  nullish: z.string().nullish().parse(undefined) === undefined,
  default: z.string().default("fallback").parse(undefined),
  defaultFunction: z.string().default(() => "factory").parse(undefined),
  defaultSkipsPresent: z.string().default(() => "factory").parse("present"),
  defaultFactorySequence: [
    countedDefaultSchema.parse(undefined),
    countedDefaultSchema.parse(undefined),
    countedDefaultSchema.parse("present"),
    defaultFactoryCalls,
  ],
  defaultTransform: z.string().transform((value) => value.length).default("tuna").parse(undefined),
  removeDefault: z.string().default("fallback").removeDefault().safeParse(undefined).success,
  catch: z.number().catch(9).parse("bad"),
  catchTransform: z.string().transform((value) => value.length).catch(9).parse(123),
  catchFunction: z.number().catch((ctx) => ctx.error.issues.length).parse("bad"),
  catchFactorySequence: [
    countedCatchSchema.parse("bad"),
    countedCatchSchema.parse(3),
    catchFactoryCalls,
  ],
  catchContext: z.number().catch((ctx) => `${ctx.input}:${ctx.error.issues[0].code}`).parse("bad"),
  removeCatch: z.number().catch(9).removeCatch().safeParse("bad").success,
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
  readonlyArray: Object.isFrozen(z.array(z.string()).readonly().parse(["a"])),
  readonlyTuple: Object.isFrozen(z.tuple([z.string()]).readonly().parse(["a"])),
  readonlyMap: Object.isFrozen(z.map(z.string(), z.number()).readonly().parse(new Map([["a", 1]]))),
  readonlySet: Object.isFrozen(z.set(z.string()).readonly().parse(new Set(["a"]))),
  readonlyMutations: {
    object: [Reflect.set(readonlyObjectValue, "id", 2), readonlyObjectValue.id],
    nestedObject: [
      Object.isFrozen(readonlyNestedValue),
      Object.isFrozen(readonlyNestedValue.nested),
      Object.isFrozen(readonlyNestedValue.tags),
      Reflect.set(readonlyNestedValue, "extra", true),
      Reflect.set(readonlyNestedValue.nested, "id", 2),
      tryErrorName(() => readonlyNestedValue.tags.push("b")),
      readonlyNestedValue,
      readonlyNestedInput,
      readonlyNestedValue.nested !== readonlyNestedInput.nested,
      readonlyNestedValue.tags !== readonlyNestedInput.tags,
    ],
    array: [
      Reflect.set(readonlyArrayValue, "0", "b"),
      tryErrorName(() => readonlyArrayValue.push("b")),
      readonlyArrayValue[0],
      readonlyArrayValue.length,
    ],
    tuple: [
      Reflect.set(readonlyTupleValue, "0", "b"),
      tryErrorName(() => readonlyTupleValue.push("b")),
      readonlyTupleValue[0],
      readonlyTupleValue.length,
    ],
    map: [
      tryErrorName(() => readonlyMapValue.set("b", 2)),
      readonlyMapValue.size,
      readonlyMapValue.get("b"),
    ],
    set: [
      tryErrorName(() => readonlySetValue.add("b")),
      readonlySetValue.size,
      readonlySetValue.has("b"),
    ],
  },
  schemaArray: z.string().array().parse(["a", "b"]).length,
  schemaOr: z.string().or(z.number()).parse(2),
  schemaAnd: z.object({ a: z.string() }).and(z.object({ b: z.number() })).parse({ a: "x", b: 1 }),
  metadata: {
    optional: [optionalMetadataSchema._def.typeName, optionalMetadataSchema._def.innerType.constructor.name],
    nullable: [nullableMetadataSchema._def.typeName, nullableMetadataSchema._def.innerType.constructor.name],
    default: [defaultMetadataSchema._def.typeName, defaultMetadataSchema._def.defaultValue()],
    catch: [catchMetadataSchema._def.typeName, catchMetadataSchema._def.catchValue({ error: null, input: "bad" })],
    promise: [promiseMetadataSchema._def.typeName, promiseMetadataSchema._def.type.constructor.name],
    readonly: [readonlyMetadataSchema._def.typeName, readonlyMetadataSchema._def.innerType.constructor.name],
    branded: [
      brandedMetadataSchema._def.typeName,
      brandedMetadataSchema.unwrap().constructor.name,
      brandedMetadataSchema instanceof z.ZodBranded,
      z.ZodBranded.name,
    ],
  },
});

type Tree = { name: string; children?: Tree[] };
const treeSchema: z.ZodType<Tree> = z.lazy(() => z.object({ name: z.string(), children: z.array(treeSchema).optional() }));
const lazyTreeInput: Tree = { name: "root", children: [{ name: "leaf" }] };
const lazyTreeParsed = treeSchema.parse(lazyTreeInput);
lazyTreeParsed.children![0].name = "changed";
print("lazy", {
  parsed: treeSchema.parse({ name: "root", children: [{ name: "leaf" }] }),
  cloneIdentity: lazyTreeParsed !== lazyTreeInput,
  childCloneIdentity: lazyTreeParsed.children![0] !== lazyTreeInput.children![0],
  source: lazyTreeInput,
  mutated: lazyTreeParsed,
});

const lateNodeSchema: z.ZodType<any> = z.late.object(() => ({
  name: z.string(),
  children: z.array(lateNodeSchema).default([]),
}));
const jsonLiteralSchema = z.union([z.string(), z.number(), z.boolean(), z.null()]);
const jsonValueSchema: z.ZodType<any> = z.lazy(() =>
  z.union([jsonLiteralSchema, z.array(jsonValueSchema), z.record(jsonValueSchema)]),
);
const jsonStringSchema = z.string().transform((value) => JSON.parse(value)).pipe(jsonValueSchema);
const mapSizeSchema = z.object({
  box: z.instanceof(Map).refine((value) => value.size === 1, { path: ["size"], message: "size one" }),
});
const mergedStaticShape = z.objectUtil.mergeShapes(
  { a: z.string(), shared: z.string() },
  { b: z.number(), shared: z.number() },
);
const effectAliasSchema = z.effect(z.string(), {
  type: "transform",
  transform: (value: string) => value.length,
});
const transformerAliasSchema = z.transformer(z.string(), {
  type: "transform",
  transform: (value: string) => value.toUpperCase(),
});
print("factories", {
  arrayFactory: z.array(z.string()).parse(["factory"]).join("|"),
  optionalFactory: z.optional(z.string()).parse(undefined) === undefined,
  nullableFactory: z.nullable(z.string()).parse(null) === null,
  promiseFactory: await z.promise(z.number()).parse(Promise.resolve(4)),
  ostringFactory: z.ostring().parse(undefined) === undefined,
  onumberFactory: z.onumber().parse(3),
  obooleanFactory: z.oboolean().parse(undefined) === undefined,
  unionFactory: z.union([z.literal("left"), z.literal("right")]).parse("right"),
  intersectionFactory: z.intersection(z.object({ a: z.string() }), z.object({ b: z.number() })).parse({ a: "x", b: 2 }),
  lazyFactory: z.lazy(() => z.string()).parse("lazy"),
  lateObject: lateNodeSchema.parse({ name: "root", children: [{ name: "leaf" }] }),
});

print("staticFactories", {
  string: z.ZodString.create().parse("x"),
  number: z.ZodNumber.create().parse(1),
  nan: Number.isNaN(z.ZodNaN.create().parse(NaN)),
  array: z.ZodArray.create(z.string()).parse(["a"]).length,
  object: z.ZodObject.create({ id: z.number() }).parse({ id: 1 }).id,
  tuple: z.ZodTuple.create([z.string(), z.number()]).parse(["a", 1])[1],
  record: z.ZodRecord.create(z.string()).parse({ a: "x" }).a,
  map: Array.from(z.ZodMap.create(z.string(), z.number()).parse(new Map([["a", 1]])).entries()),
  set: Array.from(z.ZodSet.create(z.string()).parse(new Set(["a"])).values()),
  optional: z.ZodOptional.create(z.string()).parse(undefined) === undefined,
  nullable: z.ZodNullable.create(z.string()).parse(null) === null,
  promise: await z.ZodPromise.create(z.number()).parse(Promise.resolve(2)),
  readonly: Object.isFrozen(z.ZodReadonly.create(z.object({ id: z.number() })).parse({ id: 1 })),
  effectAlias: effectAliasSchema.parse("tuna"),
  transformerAlias: transformerAliasSchema.parse("tuna"),
  objectUtil: z.object(mergedStaticShape).parse({ a: "x", b: 1, shared: 2 }),
  literal: z.ZodLiteral.create("ready").parse("ready"),
  enum: z.ZodEnum.create(["red", "blue"]).parse("blue"),
  nativeEnum: z.ZodNativeEnum.create({ A: "a", B: "b" }).parse("a"),
  date: z.ZodDate.create().parse(new Date("2020-01-02T00:00:00.000Z")).toISOString(),
  bigint: z.ZodBigInt.create().parse(2n).toString(),
  boolean: z.ZodBoolean.create().parse(true),
  any: z.ZodAny.create().parse({ ok: true }).ok,
  unknown: z.ZodUnknown.create().parse(["x"]).length,
  never: z.ZodNever.create().safeParse("x").success,
  undefined: z.ZodUndefined.create().parse(undefined) === undefined,
  null: z.ZodNull.create().parse(null) === null,
  void: z.ZodVoid.create().parse(undefined) === undefined,
  symbol: typeof z.ZodSymbol.create().parse(Symbol.for("x")),
  union: z.ZodUnion.create([z.string(), z.number()]).parse(3),
  intersection: z.ZodIntersection.create(z.object({ a: z.string() }), z.object({ b: z.number() })).parse({ a: "x", b: 1 }),
  discriminatedUnion: z.ZodDiscriminatedUnion.create("kind", [
    z.object({ kind: z.literal("a"), value: z.string() }),
    z.object({ kind: z.literal("b"), value: z.number() }),
  ]).parse({ kind: "b", value: 2 }).value,
  function: z.ZodFunction.create()
    .args(z.string())
    .returns(z.number())
    .implement((value) => value.length)("abc"),
  lazy: z.ZodLazy.create(() => z.string()).parse("lazy"),
  effects: z.ZodEffects.create(z.string(), { type: "transform", transform: (value: string) => value.length }).parse("fish"),
  default: z.ZodDefault.create(z.string(), { default: () => "fallback" }).parse(undefined),
  catch: z.ZodCatch.create(z.number(), { catch: () => 7 }).parse("bad"),
  pipeline: z.ZodPipeline.create(z.string(), z.coerce.number()).parse("6"),
});

print("jsonLike", {
  object: jsonValueSchema.safeParse({ nested: [1, "two", null] }).success,
  rejectsFunction: jsonValueSchema.safeParse(() => 1).success,
  rejectsSymbol: jsonValueSchema.safeParse(Symbol.for("x")).success,
  rejectsBigint: jsonValueSchema.safeParse(1n).success,
  badNested: issueMetadataFromResult(jsonValueSchema.safeParse({ ok: true, bad: () => 1 })),
  fromString: jsonStringSchema.parse('{"a":[1,true,null]}'),
  mapSize: mapSizeSchema.safeParse({ box: new Map([["a", 1]]) }).success,
  mapSizeFail: issueMetadataFromResult(mapSizeSchema.safeParse({ box: new Map() })),
  parsed: jsonValueSchema.parse({ ok: true, items: [1, "two"] }),
});

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
const strictValidatedFn = z.function().args(z.string()).returns(z.number()).strictImplement((value) => value.trim().length);
const invalidReturnFn = z.function().args(z.string()).returns(z.number()).implement(() => "bad" as unknown as number);
const strictInvalidReturnFn = z.function().args(z.string()).returns(z.number()).strictImplement(() => "bad" as unknown as number);
const validatedAliasFn = z.function().args(z.string()).returns(z.number()).validate((value) => value.trim().length);
const restValidatedFn = z
  .function(z.tuple([z.string()]).rest(z.number()), z.number())
  .implement((label, ...values) => label.length + values.reduce((total, value) => total + value, 0));
const functionSchema = z.function().args(z.string(), z.number()).returns(z.boolean());
const functionMetadataSchema = z.function().args(z.string(), z.number()).returns(z.boolean());
const defaultFunctionMetadataSchema = z.function();
const argsOnlyFunctionMetadataSchema = z.function().args(z.string());
const returnsOnlyFunctionMetadataSchema = z.function().returns(z.number());
const asyncValidatedFn = z.function().args(z.string()).returns(z.promise(z.number())).implement(async (value) => value.trim().length);
const asyncInvalidReturnFn = z.function().args(z.string()).returns(z.promise(z.number())).implement(async () => "bad" as unknown as number);
const functionCloneInput = { nested: { id: 1 } };
let functionCloneSeenArg: { nested: { id: number } } | undefined;
const functionCloneValidatedFn = z
  .function()
  .args(z.object({ nested: z.object({ id: z.number() }) }))
  .returns(z.object({ nested: z.object({ id: z.number() }) }))
  .implement((value) => {
    functionCloneSeenArg = value;
    value.nested.id = 2;
    return value;
  });
const functionCloneReturned = functionCloneValidatedFn(functionCloneInput);
functionCloneReturned.nested.id = 3;
const functionNestedIssueCount = (issue: z.ZodIssue) => {
  if (issue.code === "invalid_arguments") {
    return issue.argumentsError.issues.length;
  }
  if (issue.code === "invalid_return_type") {
    return issue.returnTypeError.issues.length;
  }
  return 0;
};
const functionIssueSummary = (call: () => unknown) => {
  try {
    call();
    return [];
  } catch (error) {
    return (error as z.ZodError).issues.map((issue) => ({
      code: issue.code,
      path: issue.path.join("."),
      nestedCount: functionNestedIssueCount(issue),
    }));
  }
};
const functionArgumentIssuePaths = (call: () => unknown) => {
  try {
    call();
    return [];
  } catch (error) {
    const issue = (error as z.ZodError).issues[0];
    return issue.code === "invalid_arguments"
      ? issue.argumentsError.issues.map((nestedIssue) => `${nestedIssue.path.join(".")}:${nestedIssue.code}`)
      : [];
  }
};
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
  strictValid: strictValidatedFn(" trout "),
  validateAlias: validatedAliasFn(" bass "),
  restValid: restValidatedFn("ab", 1, 2, 3),
  restInvalid: functionArgumentIssuePaths(() => (restValidatedFn as unknown as (...values: unknown[]) => number)("ab", 1, "x")),
  asyncValid: await asyncValidatedFn(" salmon "),
  cloneSemantics: {
    argIdentity: functionCloneSeenArg !== functionCloneInput,
    nestedArgIdentity: functionCloneSeenArg?.nested !== functionCloneInput.nested,
    source: functionCloneInput,
    returnIdentity: functionCloneReturned !== functionCloneSeenArg,
    nestedReturnIdentity: functionCloneReturned.nested !== functionCloneSeenArg?.nested,
    returned: functionCloneReturned,
  },
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
  argumentIssues: functionIssueSummary(() => (validatedFn as unknown as (value: number) => number)(1)),
  returnIssues: functionIssueSummary(() => invalidReturnFn("x")),
  strictReturnIssues: functionIssueSummary(() => strictInvalidReturnFn("x")),
  invalidAsyncReturn,
  metadata: {
    typed: {
      typeName: functionMetadataSchema._def.typeName,
      argsType: functionMetadataSchema._def.args.constructor.name,
      argItems: functionMetadataSchema._def.args.items.map((item) => item.constructor.name),
      argRest: functionMetadataSchema._def.args._def.rest?.constructor.name,
      returns: functionMetadataSchema._def.returns.constructor.name,
      parameters: functionMetadataSchema.parameters().items.map((item) => item.constructor.name),
      returnType: functionMetadataSchema.returnType().constructor.name,
    },
    defaults: {
      args: defaultFunctionMetadataSchema._def.args.items.length,
      rest: defaultFunctionMetadataSchema._def.args._def.rest?.constructor.name,
      returns: defaultFunctionMetadataSchema._def.returns.constructor.name,
    },
    argsOnly: {
      args: argsOnlyFunctionMetadataSchema._def.args.items.map((item) => item.constructor.name),
      returns: argsOnlyFunctionMetadataSchema._def.returns.constructor.name,
    },
    returnsOnly: {
      args: returnsOnlyFunctionMetadataSchema._def.args.items.length,
      returns: returnsOnlyFunctionMetadataSchema._def.returns.constructor.name,
    },
  },
});

const asyncSchema = z.string().refine(async (value) => value === "ok");
const asyncTransformSchema = z.string().transform(async (value) => value.trim().length);
const asyncPipeSchema = z
  .string()
  .transform(async (value) => value.length)
  .pipe(z.number().min(2));
const asyncSuperRefineSchema = z.string().superRefine(async (value, ctx) => {
  if (value !== "ok") {
    ctx.addIssue({ code: z.ZodIssueCode.custom, message: "bad async" });
  }
});
const asyncPreprocessSchema = z.preprocess(
  async (value) => (typeof value === "string" ? value.trim() : value),
  z.string().min(2),
);
const numericPreprocessSchema = z.preprocess(
  (value) => (typeof value === "string" ? Number(value) : value),
  z.number().min(2),
);
const objectPipelineSchema = z.pipeline(
  z.string().transform((value) => ({ len: value.length })),
  z.object({ len: z.number().min(2) }),
);
const transformIssueSchema = z.string().transform((value, ctx) => {
  ctx.addIssue({ code: z.ZodIssueCode.custom, message: "warn" });
  return value.length;
});
const asyncPromiseMetadataSchema = z.promise(z.number());
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
const asyncSuperRefineResult = await asyncSuperRefineSchema.safeParseAsync("bad");
const asyncRefineMessageResult = await z
  .string()
  .refine(async (value) => value === "ok", { message: "not ok" })
  .safeParseAsync("bad");
print("asyncEffects", {
  transform: await asyncTransformSchema.parseAsync(" tuna "),
  pipe: await asyncPipeSchema.safeParseAsync("abc"),
  superRefineMessage: asyncSuperRefineResult.success ? "ok" : asyncSuperRefineResult.error.issues[0].message,
  refineMessage: asyncRefineMessageResult.success ? "ok" : asyncRefineMessageResult.error.issues[0].message,
  preprocess: await asyncPreprocessSchema.parseAsync(" ok "),
  syncThrows: {
    transformParse: thrownSummary(() => asyncTransformSchema.parse("x")),
    transformSafeParse: thrownSummary(() => asyncTransformSchema.safeParse("x")),
    refineParse: thrownSummary(() => asyncSchema.parse("bad")),
    refineSafeParse: thrownSummary(() => asyncSchema.safeParse("bad")),
  },
  metadata: {
    refine: [asyncSchema._def.typeName, asyncSchema._def.effect.type, asyncSchema._def.schema.constructor.name],
    transform: [
      asyncTransformSchema._def.typeName,
      asyncTransformSchema._def.effect.type,
      asyncTransformSchema._def.schema.constructor.name,
    ],
    pipe: [asyncPipeSchema._def.typeName, asyncPipeSchema._def.in.constructor.name, asyncPipeSchema._def.out.constructor.name],
    superRefine: [
      asyncSuperRefineSchema._def.typeName,
      asyncSuperRefineSchema._def.effect.type,
      asyncSuperRefineSchema._def.schema.constructor.name,
    ],
    preprocess: [
      asyncPreprocessSchema._def.typeName,
      asyncPreprocessSchema._def.effect.type,
      asyncPreprocessSchema._def.schema.constructor.name,
    ],
    promise: [asyncPromiseMetadataSchema._def.typeName, asyncPromiseMetadataSchema._def.type.constructor.name],
  },
});
print("asyncIssues", {
  parseAsync: issueMetadataFromResult(await z.string().min(2).safeParseAsync("x")),
  pipe: issueMetadataFromResult(await asyncPipeSchema.safeParseAsync("x")),
  preprocess: issueMetadataFromResult(await asyncPreprocessSchema.safeParseAsync(" x ")),
  promiseInner: await issueMetadataFromThrown(() => z.promise(z.number()).parse(Promise.resolve("bad"))),
});

const promiseSchema = z.promise(z.number());
const promiseSafeResult = promiseSchema.safeParse(Promise.resolve(8));
const promiseObjectValue = await z.promise(z.object({ id: z.number() })).parse(Promise.resolve({ id: 1 }));
const promiseCloneInput = { nested: { id: 1 } };
const promiseCloneParsed = await z
  .promise(z.object({ nested: z.object({ id: z.number() }) }))
  .parse(Promise.resolve(promiseCloneInput));
promiseCloneParsed.nested.id = 2;
print("promiseSchemas", {
  safeParse: promiseSafeResult.success,
  safeValue: promiseSafeResult.success ? await promiseSafeResult.data : null,
  parseThen: typeof promiseSchema.parse(Promise.resolve(2)).then,
  parseValue: await promiseSchema.parse(Promise.resolve(2)),
  nonPromise: issueMetadataFromResult(promiseSchema.safeParse(1)),
  inner: await issueMetadataFromThrown(() => promiseSchema.parse(Promise.resolve("x"))),
  object: promiseObjectValue,
  cloneIdentity: promiseCloneParsed !== promiseCloneInput,
  cloneNestedIdentity: promiseCloneParsed.nested !== promiseCloneInput.nested,
  cloneSource: promiseCloneInput,
  cloneParsed: promiseCloneParsed,
  methodType: z.number().promise()._def.type.constructor.name,
});

print("effectFactories", {
  preprocessNumber: numericPreprocessSchema.parse("3"),
  preprocessIssue: issueMetadataFromResult(numericPreprocessSchema.safeParse("1")),
  pipelineObject: objectPipelineSchema.parse("abc"),
  pipelineIssue: issueMetadataFromResult(objectPipelineSchema.safeParse("a")),
  transformIssue: issueMetadataFromResult(transformIssueSchema.safeParse("abc")),
});

const formatted = objectBase.safeParse({ id: "x", name: 1 });
if (!formatted.success) {
  const nestedFormatted = z.object({ user: z.object({ tags: z.array(z.string().min(2)).min(2) }) }).safeParse({ user: { tags: ["x"] } });
  const unionFailure = z
    .union([
      z.object({ kind: z.literal("a"), value: z.string() }),
      z.object({ kind: z.literal("b"), count: z.number() }),
    ])
    .safeParse({ kind: "a", value: 1 });
  const discriminatedFailure = eventSchema.safeParse({ type: "missing", value: true });
  const unionIssue = unionFailure.success ? null : unionFailure.error.issues[0];
  const discriminatedIssue = discriminatedFailure.success ? null : discriminatedFailure.error.issues[0];
  print("errors", {
    formatKeys: Object.keys(formatted.error.format()).sort(),
    flatten: formatted.error.flatten(),
    nestedTagErrors: nestedFormatted.success ? [] : nestedFormatted.error.format().user?.tags?._errors,
    nestedTagItemErrors: nestedFormatted.success ? [] : nestedFormatted.error.format().user?.tags?.[0]?._errors,
    unionIssue: unionIssue?.code,
    unionBranchIssueCounts: unionIssue?.code === "invalid_union" ? unionIssue.unionErrors.map((error) => error.issues.length) : [],
    unionFirstBranchPath: unionIssue?.code === "invalid_union" ? unionIssue.unionErrors[0].issues[0].path.join(".") : "",
    discriminatedIssue: discriminatedIssue?.code,
    discriminatedPath: discriminatedIssue?.path.join("."),
  });
}

print("issueMetadata", {
  literal: issueMetadataFromResult(z.literal("ready").safeParse("no")),
  invalidType: issueMetadataFromResult(z.number().safeParse("x")),
  enum: issueMetadataFromResult(z.enum(["red", "blue"]).safeParse("green")),
  invalidString: issueMetadataFromResult(z.string().email().safeParse("bad")),
  invalidRegex: issueMetadataFromResult(z.string().regex(/^a+$/).safeParse("bbb")),
  strict: issueMetadataFromResult(z.object({ id: z.number() }).strict().safeParse({ id: 1, extra: true })),
  invalidDate: issueMetadataFromResult(z.date().safeParse(new Date("bad"))),
  notFinite: issueMetadataFromResult(z.number().finite().safeParse(Infinity)),
  dateTooSmall: issueMetadataFromResult(
    z.date().min(new Date("2020-01-02T00:00:00.000Z")).safeParse(new Date("2020-01-01T00:00:00.000Z")),
  ),
  dateTooBig: issueMetadataFromResult(
    z.date().max(new Date("2020-01-02T00:00:00.000Z")).safeParse(new Date("2020-01-03T00:00:00.000Z")),
  ),
  tooBigArray: issueMetadataFromResult(z.array(z.string()).max(1).safeParse(["a", "b"])),
  tooSmallNumber: issueMetadataFromResult(z.number().min(2).safeParse(1)),
  exactArray: issueMetadataFromResult(z.array(z.string()).length(2).safeParse(["a"])),
  exactString: issueMetadataFromResult(z.string().length(2).safeParse("a")),
  notMultiple: issueMetadataFromResult(z.number().multipleOf(5).safeParse(12)),
  invalidIntersection: issueMetadataFromResult(
    z.intersection(z.string().transform(() => 1), z.string().transform(() => 2)).safeParse("x"),
  ),
  custom: issueMetadataFromResult(
    z.string()
      .superRefine((value, ctx) => {
        ctx.addIssue({ code: z.ZodIssueCode.custom, message: `custom:${value}`, params: { kind: "fixture" } });
      })
      .safeParse("x"),
  ),
});

const manualError = new z.ZodError([]);
manualError.addIssue({ code: z.ZodIssueCode.custom, path: ["manual"], message: "manual issue" });
manualError.addIssues([{ code: z.ZodIssueCode.custom, path: ["more"], message: "more issue" }]);
const createdError = z.ZodError.create([
  { code: z.ZodIssueCode.custom, path: ["create"], message: "created issue" },
]);
const mappedFormat = objectBase.safeParse({ id: "x", name: 1 });
const unionFormatted = z
  .union([z.object({ a: z.string() }), z.object({ b: z.number() })])
  .safeParse({ a: 1, b: "x" });
print("errorMethods", {
  manualCount: manualError.issues.length,
  manualEmpty: manualError.isEmpty,
  manualMessage: manualError.message.includes("manual issue"),
  manualToString: manualError.toString().includes("more issue"),
  manualErrorsAlias: manualError.errors === manualError.issues,
  manualFormErrors: manualError.formErrors.fieldErrors.manual?.[0],
  manualFlatten: manualError.flatten((issue) => `${issue.path.join(".")}:${issue.message}`),
  created: createdError.issues[0].message,
  mappedFormat: mappedFormat.success
    ? null
    : mappedFormat.error.format((issue) => `${issue.code}:${issue.message}`).id?._errors,
  unionFormat: unionFormatted.success
    ? null
    : unionFormatted.error.format((issue) => `${issue.path.join(".")}:${issue.code}`),
  unionFlatten: unionFormatted.success
    ? null
    : unionFormatted.error.flatten((issue) => `${issue.path.join(".")}:${issue.code}`),
});

const formattingResult = z
  .object({
    user: z.object({
      name: z.string(),
      tags: z.array(z.string().min(2)).min(2),
    }),
  })
  .strict()
  .safeParse({ user: { name: 1, tags: ["x"] }, extra: true });
if (!formattingResult.success) {
  const formattedError = formattingResult.error.format();
  print("errorFormatting", {
    issueCodes: formattingResult.error.issues.map((issue) => issue.code),
    formatKeys: Object.keys(formattedError.user ?? {}).sort(),
    formatRoot: formattedError._errors,
    formatName: formattedError.user?.name?._errors,
    formatTags: formattedError.user?.tags?._errors,
    formatTag0: formattedError.user?.tags?.[0]?._errors,
    flatten: formattingResult.error.flatten(),
    flattenMapped: formattingResult.error.flatten((issue) => `${issue.path.join(".")}:${issue.code}`),
    formErrors: formattingResult.error.formErrors,
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

const parseOptionsObjectResult = z.object({ item: z.string().min(2) }).safeParse({ item: "x" }, { path: ["root"] });
const parseOptionsUnionResult = z.union([z.string(), z.number()]).safeParse(true, {
  path: ["value"],
  errorMap: (issue, ctx) => ({ message: `union:${issue.code}:${ctx.defaultError}` }),
});
const parseOptionsAsyncResult = await z.string().min(2).safeParseAsync("x", {
  path: ["asyncRoot"],
  errorMap: (issue) => ({ message: `async:${issue.code}` }),
});
print("parseOptions", {
  object: issueMetadataFromResult(parseOptionsObjectResult),
  union: issueMetadataFromResult(parseOptionsUnionResult),
  async: issueMetadataFromResult(parseOptionsAsyncResult),
});

const summarySchema = z.array(userSchema.pick({ id: true, role: true })).min(1);
print("array", summarySchema.parse([{ id: 1, role: "admin" }]));

const standardOk = objectBase["~standard"].validate({ id: 1, name: "a" });
const standardBad = objectBase["~standard"].validate({ id: "x", name: 1 });
const legacyStandardOk = (objectBase as any)["~validate"]({ id: 1, name: "a" });
const legacyStandardBad = (objectBase as any)["~validate"]({ id: "x", name: 1 });
const legacyAsyncStandardOk = await (z.string().refine(async (value) => value === "ok") as any)["~validate"]("ok");
const legacyAsyncStandardBad = await (z.string().refine(async (value) => value === "ok") as any)["~validate"]("bad");
print("standard", {
  vendor: objectBase["~standard"].vendor,
  version: objectBase["~standard"].version,
  ok: "value" in standardOk ? standardOk.value : null,
  badIssues: "issues" in standardBad ? standardBad.issues?.length : 0,
  legacyOk: "value" in legacyStandardOk ? legacyStandardOk.value : null,
  legacyBadIssues: "issues" in legacyStandardBad ? legacyStandardBad.issues?.map((issue: z.ZodIssue) => `${issue.path.join(".")}:${issue.code}`) : [],
  legacyAsyncOk: "value" in legacyAsyncStandardOk ? legacyAsyncStandardOk.value : null,
  legacyAsyncBad: "issues" in legacyAsyncStandardBad ? legacyAsyncStandardBad.issues?.[0].message : null,
});

const z3SubpathResult = z3.object({ id: z3.number(), name: z3.string().default("subpath") }).parse({ id: 1 });
const z3SubpathBad = z3.object({ id: z3.number() }).safeParse({ id: "bad" });
print("packageSubpaths", {
  v3Default: z3SubpathResult.name,
  v3Issue: z3SubpathBad.success ? "ok" : z3SubpathBad.error.issues[0].code,
  v3DefaultEqualsNamed: z3Default === z3,
  v3NamespaceHasZ: z3Namespace.z === z3,
  v3NamespaceDefaultEqualsNamed: z3Namespace.default === z3,
  v3NamespaceParse: z3Namespace.string().parse("v3 namespace"),
});

const originalErrorMap = z.getErrorMap();
z.setErrorMap((issue, ctx) => ({ message: `global:${issue.code}:${ctx.defaultError}` }));
const mappedError = z.string().safeParse(1);
const schemaMappedError = z.string({ invalid_type_error: "schema invalid" }).safeParse(1);
const localMappedError = z.string().safeParse(1, {
  errorMap: (issue, ctx) => ({ message: `local:${issue.code}:${ctx.defaultError}` }),
});
z.setErrorMap(originalErrorMap);
print("errorMap", {
  mapped: mappedError.success ? "ok" : mappedError.error.issues[0].message,
  schema: schemaMappedError.success ? "ok" : schemaMappedError.error.issues[0].message,
  local: localMappedError.success ? "ok" : localMappedError.error.issues[0].message,
  restored: z.getErrorMap() === originalErrorMap,
});
