// Cross-module API registry built at MODULE INIT time.
//
// The shape is taken from #7154's disassembly: a cross-module call whose first
// arguments are string literals parked in registers across an object
// allocation, a schema build (library code with its own allocations) and a
// closure allocation. Every argument here is live across the evaluation of the
// ones that follow it, which is the "evaluate-then-allocate" hazard in
// docs/src/internals/gc-rooting-invariant.md.
import * as z from "../../node_modules/zod/src/index.js";

export interface ApiCall {
  readonly url: string;
  readonly method: string;
  readonly opts: Record<string, unknown>;
  readonly tags: string[];
  readonly describe: () => string;
}

const REGISTRY: ApiCall[] = [];

const ABSOLUTE_RE = /^https?:\/\/[a-z0-9.-]+\//;
const PATH_SEGMENT_RE = /\{([a-zA-Z_][a-zA-Z0-9_]*)\}/g;

export function defineApiCall(
  url: string,
  method: string,
  opts: Record<string, unknown>,
  tags: string[],
  schema: unknown,
  cb: (body: unknown) => string,
): ApiCall {
  // js_regexp_new allocates, then the subject string is coerced (another
  // allocation) before js_regexp_test consumes both.
  const absolute = ABSOLUTE_RE.test(String(url));
  const params: string[] = [];
  let m: RegExpExecArray | null;
  PATH_SEGMENT_RE.lastIndex = 0;
  while ((m = PATH_SEGMENT_RE.exec(url)) !== null) {
    params.push(m[1]);
  }
  // A spread into a fresh array: js_array_alloc followed by
  // js_array_spread_append, which is the single largest population in the
  // dependency-scale report.
  const allTags = [...tags, method.toLowerCase(), absolute ? "abs" : "rel"];
  const call: ApiCall = {
    url,
    method,
    opts,
    tags: allTags,
    describe(): string {
      // A boxed mutable capture read back across a closure call.
      let n = 0;
      const bump = (): number => ++n;
      bump();
      bump();
      const shown = params.length > 0 ? params.join(",") : "-";
      return `${method} ${url} [${allTags.join("|")}] {${shown}} #${n}`;
    },
  };
  REGISTRY.push(call);
  // Keep the schema reachable so the parse loop below has something to run.
  SCHEMAS.set(url + " " + method, schema);
  CALLBACKS.set(url + " " + method, cb);
  return call;
}

export const SCHEMAS = new Map<string, unknown>();
export const CALLBACKS = new Map<string, (body: unknown) => string>();

export function allApiCalls(): ApiCall[] {
  return REGISTRY.slice();
}

export function baseFields() {
  return {
    id: z.string().min(1).max(64),
    kind: z.string(),
    count: z.number().int(),
    ratio: z.number(),
    active: z.boolean(),
    labels: z.array(z.string()),
  };
}
