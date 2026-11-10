import { Inspect } from "./keys.ts"

export const PipeProto = {
  pipe(this: any) {
    return "pipe:" + this.value
  },
  toJSON(this: any) {
    return { ...this }
  },
  [Inspect](this: any) {
    return "inspect:" + this.value
  }
}

// Mutated after its literal: a spread of it holds `extra` too, which the
// compiler's prediction of the spread does not.
export const Mutated: any = { m() { return "m" } }
Mutated.extra = "extra"
