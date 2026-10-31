//! zod v4 `core/compile.ts` with its `doc.ts`, reduced (ddmin over
//! declarations, then lines; the other imports are stubs): the record check
//! hands `emitOwnKeys` a body that compiles the value through
//! `compileChild`, and `emitOwnKeys` hands `doc.indented` a closure calling
//! that body. Per unit, each unit described the other unit's closures inside
//! its own (the calls through `doc` carry them) and forwarded the copies it
//! read whole: every round nested the descriptions one level deeper (d_zod4:
//! the `emitOwnKeys` argument contract 2.2 MB at round 4, 646 MB at round 12,
//! the 40 GB cap at round 16), and this reduction never reaches a fixpoint.
//! A forwarded copy names the functions their owners describe (DECISIONS
//! 106), so the rounds end.

use super::*;

const DOC: &str = r#"
type ModeWriter = (doc: Doc, modes: { execution: "sync" | "async" }) => void;

export class Doc {
  args: string[];
  closed: Record<string, unknown>;
  content: string[] = [];
  indent = 0;

  constructor(args: string[] = [], closed: Record<string, unknown> = {}) {
    this.args = args;
    this.closed = closed;
  }

  indented(fn: (doc: Doc) => void) {
    this.indent += 1;
    try {
      fn(this);
    } finally {
      this.indent -= 1;
    }
  }

  write(fn: ModeWriter): void;
  write(line: string): void;
  write(arg: any) {
    if (typeof arg === "function") {
      (arg as ModeWriter)(this, { execution: "sync" });
      (arg as ModeWriter)(this, { execution: "async" });
      return;
    }

    const content = arg as string;
    const lines = content.split("\n").filter((x) => x);
    const minIndent = Math.min(...lines.map((x) => x.length - x.trimStart().length));
    const dedented = lines.map((x) => x.slice(minIndent)).map((x) => " ".repeat(this.indent * 2) + x);
    for (const line of dedented) {
      this.content.push(line);
    }
  }

  compile(): any {
    const F = Function;
    const content = this?.content ?? [``];
    const factory = new F(
      ...Object.keys(this.closed),
      `return function (${this.args.join(", ")}) {\n${content.join("\n")}\n};`
    );
    return factory(...Object.values(this.closed)) as any;
  }
}
"#;

const STUBS: &str = r#"
export class $ZodAsyncError extends Error {}
export function isBackEdge(a: any, b?: any): boolean { return false; }
export function isRecursiveSchema(a: any): boolean { return false; }
export function isValidBase64(s: string) { return s.length > 0; }
export function isValidBase64URL(s: string) { return s.length > 0; }
export function isValidCIDRv6(s: string) { return s.length > 0; }
export function isValidCreditCard(s: string) { return s.length > 0; }
export function isValidIBAN(s: string) { return s.length > 0; }
export function isValidIPv6(s: string) { return s.length > 0; }
export function isValidJWT(s: string, a?: any) { return s.length > 0; }
export function mergeValues(a: any, b: any) { return { valid: true, data: a }; }
export function stripTabAndNewline(s: string) { return s; }
export function urlHostnameOk(a: any, b: any) { return true; }
export function urlProtocolOk(a: any, b: any) { return true; }
export function validateURL(a: any) { return a; }
"#;

const STUBS_RE: &str = r#"
export const number = /^-?\d+(?:\.\d+)?$/;
"#;

const STUBS_UTIL: &str = r#"
export const CONSTANT_CATCH = Symbol("c");
export type JWTAlgorithm = string;
export type Primitive = string | number;
export const NUMBER_FORMAT_RANGES: Record<string, [number, number]> = { int32: [-2147483648, 2147483647] };
export function clone(a: any, b?: any) { return a; }
export function codePointLength(s: string) { return s.length; }
export function esc(s: any) { return JSON.stringify(s); }
export function floatSafeRemainder(a: number, b: number) { return a % b; }
export function isPlainObject(a: any) { return typeof a === "object" && a !== null; }
export function shallowClone(a: any) { return { ...a }; }
"#;

const COMPILE: &str = r#"
import { Doc } from "./doc";
import * as regexes from "./stubs_re";
import * as util from "./stubs_util";
import { $ZodAsyncError, isBackEdge, isRecursiveSchema, isValidBase64, isValidBase64URL, isValidCIDRv6, isValidCreditCard, isValidIBAN, isValidIPv6, isValidJWT, mergeValues, stripTabAndNewline, urlHostnameOk, urlProtocolOk, validateURL } from "./stubs";
type SomeType = any; type ParseContextInternal = any; type ParsePayload = any;
declare namespace checks { type $ZodCheckNumberFormatDef = any; type $ZodCheckBigIntFormatDef = any; }
declare namespace core { type output<T> = any; }
export function compileFn<T extends SomeType>(schema: T, options?: CompileFnOptions): CompiledFn<core.output<T>> {
  try {
  } catch {}
  const ctx: CompileContext = {
  };
  const outputAccessor = generateCheck(doc, ctx, schema, "input", !options?.assertOnly);
}
function compileChild(
  doc: Doc,
): string | null {
  const constantCount = ctx.constants.size;
  const constantCounter = ctx.constantCounter;
  const varCounter = ctx.varCounter;
  try {
    return generateCheck(doc, ctx, schema, accessor, needsValue);
  } catch (err) {
    if (!(err instanceof ZodCompileUnsupportedError) || !err.islandable) throw err;
    ctx.varCounter = varCounter;
    return emitRuntimeIsland(doc, ctx, schema, accessor);
  }
}

function emitRuntimeIsland(doc: Doc, ctx: CompileContext, schema: SomeType, accessor: string): string {
  for (const check of schemaChecks) {
    const def = check._zod.def;
    switch (def.check) {
      case "greater_than":
        break;
        break;
      case "overwrite": {
        void (def satisfies never);
        throw new ZodCompileUnsupportedError(`check type ${(def as { check: string }).check}`);
      }
    }
  }

  return currentAccessor;
}

function generateLessThanCheck(
  doc: Doc,
): void {
  return undefined as any;
}
function generatePropertyCheck(
  accessor: string
): void {
  return undefined as any;
}
function generateStringFormatCheck(
  doc: Doc,
  needsValue = true
): string | null {
  return undefined as any;
}
function generateCheck(
  doc: Doc,
  needsValue = true
): string | null {
  const def = schema._zod.def;
  let typeAccessor: string | null;

  switch (type) {
    case "string":
      typeAccessor = "true";
      break;
      typeAccessor = generateRecordCheck(doc, ctx, schema, accessor);
      break;
    case "map":
    default: {
      void (type satisfies never);
      throw new ZodCompileUnsupportedError(`schema type ${type}`);
    }
  }

  return undefined as any;
}
function generateObjectCheck(
  doc: Doc,
  ctx: CompileContext,
): string | null {
  return undefined as any;
}
function generateArrayCheck(
  doc: Doc,
  ctx: CompileContext,
): string | null {
  return undefined as any;
}
function generateUnionCheck(doc: Doc, ctx: CompileContext, schema: SomeType, accessor: string): string {
  const def = schema._zod.def as unknown as {
    options: SomeType[];
  };
}

function generateDiscriminatedUnionCheck(
  doc: Doc,
): string {
  return undefined as any;
}
function generateRecordCheck(doc: Doc, ctx: CompileContext, schema: SomeType, accessor: string): string {
  const def = schema._zod.def as unknown as { keyType: SomeType; valueType: SomeType };

  const keyValues = recordDef.partial ? undefined : (def.keyType._zod as unknown as { values?: Set<unknown> }).values;
  if (keyValues) {
    const inputKeys: Array<string | symbol> = [];
    for (const key of keyValues) {
      const keyConst = addConstant(ctx, key);
      const outKey = generateCheck(doc, ctx, def.keyType, keyConst);
      const valOutput = compileChild(doc, ctx, def.valueType, valueVar);
      doc.write(`${outputVar}[${outKey}] = ${valOutput};`);
    }
  }

    keyDef.type === "string" && keyDef.format === undefined && !keyDef.coerce && (keyDef.checks?.length ?? 0) === 0;
  if (!keyIsBareString) {
    const isLoose = (def as { mode?: string }).mode === "loose";
    const keyFn = compileFn(def.keyType);
    emitOwnKeys(doc, ctx, accessor, kVar, (d) => {
      d.write(`let ${outKeyVar} = ${keyFast}(${kVar});`);
      d.write(`${outputVar}[${outKeyVar}] = ${valOutput};`);
    });
    return outputVar;
  }

  emitOwnKeys(
    doc,
    ctx,
    accessor,
    kVar,
    (d) => {
      d.write(`const ${valVar} = ${accessor}[${kVar}];`);
      const valOutput = compileChild(d, ctx, def.valueType, valVar);
      d.write(`${outputVar}[${kVar}] = ${valOutput};`);
    },
    `return INVALID;`
  );

  return outputVar;
}

function emitOwnKeys(
  doc: Doc,
  ctx: CompileContext,
  accessor: string,
  kVar: string,
  body: (d: Doc) => void,
  onSymbol?: string
): void {
  const propIsEnumerableConst = addConstant(ctx, Object.prototype.propertyIsEnumerable);
  const symsVar = newVar(ctx);
  doc.indented((d) => {
    d.write(`const ${kVar} = ${symsVar}[${iVar}];`);
    d.write(`if (!${propIsEnumerableConst}.call(${accessor}, ${kVar})) continue;`);
    if (onSymbol) d.write(onSymbol);
    else body(d);
  });
"#;

const MAIN: &str = r#"
import { compileFn } from "./compile";
const s: any = { _zod: { def: { type: "record", keyType: { _zod: { def: { type: "string" } } }, valueType: { _zod: { def: { type: "string" } } } } } };
try { console.log(String(compileFn(s))); } catch (e) { console.log("err", String(e)); }
"#;

#[test]
fn forwarded_function_copies_stay_bounded() {
    let mods: &'static [(&'static str, &'static str)] = &[("doc", DOC), ("stubs", STUBS), ("stubs_re", STUBS_RE), ("stubs_util", STUBS_UTIL), ("compile", COMPILE), ("main", MAIN)];
    // Before: the rounds never end, so the bound is a wait.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(units_of(mods));
    });
    let pf = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("per-unit facts reach a fixpoint");
    assert!(pf.rounds <= 8, "rounds {}", pf.rounds);
}
