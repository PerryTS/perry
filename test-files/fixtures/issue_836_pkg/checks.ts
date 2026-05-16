// Issue #836 fixture: producer module mirroring zod's
// `node_modules/zod/src/v4/core/checks.ts`. The key shape is
// `export const $Name = ...` — a value-export whose name contains `$`.
// `sanitize()` on the producer side rewrites `$` to `_`, while the
// consumer side reads the origin name VERBATIM. Pre-fix the producer
// emitted `perry_fn_<src>___ZodCheck` but the consumer linker call
// site was `perry_fn_<src>__$ZodCheck`, and the link failed.

export const $ZodCheck = {
  kind: "check",
  validate(x: number): boolean {
    return x >= 0;
  },
};

export const $ZodCheckStringFormat = {
  kind: "string-format",
  validate(s: string): boolean {
    return typeof s === "string" && s.length > 0;
  },
};
