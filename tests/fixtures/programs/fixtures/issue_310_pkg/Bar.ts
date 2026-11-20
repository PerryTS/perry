// Bar "module" — second namespace re-export target so we exercise the
// multi-namespace-in-one-package shape (parallel to Effect / Cause /
// Layer / etc. all re-exported from `effect/src/index.ts`).

export function greet(name: string): string {
  return "hello, " + name;
}

export function shout(text: string): string {
  return text + "!";
}
