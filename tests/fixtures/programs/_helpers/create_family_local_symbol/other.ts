// The same description as the main module's key: a different symbol.
export const key = Symbol("schema")
export const OtherProto: any = { _tag: "Other", [key]: "other", step(x: number) { return x * 10 } }
export function other(v: number) {
  const a = Object.create(OtherProto)
  a.value = v
  let s = 0
  for (let i = 0; i < 4; i++) s += a.step(i) + (a[key] === "other" ? 1 : 0)
  return s + ":" + a._tag
}
