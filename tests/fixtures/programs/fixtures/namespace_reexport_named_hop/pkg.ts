// The namespace target. The `const` alias of a local function is load-bearing:
// with `export function t(k) {...}` or `export const t = 42` instead, the
// consumer resolves the member statically and the chain never matters.
export function translate(k: string): string {
  return "T:" + k;
}
export const t = translate;
