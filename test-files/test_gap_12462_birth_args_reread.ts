// #12462: an object literal / fixed-layout class born in one inline step must
// store the CURRENT value of each argument. `{ _tag: tag(..), annotations:
// optional(..), meta }` lowers `tag(..)` first; `optional(..)` then allocates
// enough to run a moving minor collection. The compact birth stored the
// register `tag(..)` produced instead of re-reading its root, so `_tag` held
// the object's pre-collection (from-space) address.
class Tag {
  literal: string
  pad: number[]
  constructor(literal: string, pad: number[]) {
    this.literal = literal
    this.pad = pad
  }
}
class Opt {
  inner: any
  extra: number[]
  constructor(inner: any, extra: number[]) {
    this.inner = inner
    this.extra = extra
  }
}
class Pair {
  left: any
  right: any
  constructor(left: any, right: any) {
    this.left = left
    this.right = right
  }
}

function tag(s: string): Tag {
  return new Tag(s, [1, 2, 3])
}
function churn(n: number): number {
  let t = 0
  for (let i = 0; i < n; i++) {
    const a = { i, s: "x" + i, arr: [i, i + 1] }
    t += a.arr.length
  }
  return t
}
function optional(x: any): Opt {
  churn(2000)
  return new Opt(x, [4, 5])
}
const ann = { a: 1 }

function literalBirth(meta: any, id: string) {
  return { _tag: tag("Filter" + id), annotations: optional(ann), meta }
}
function classBirth(id: string): Pair {
  return new Pair(tag("Left" + id), optional(ann))
}

let bad = 0
const keep: any[] = []
const pairs: Pair[] = []
for (let i = 0; i < 300; i++) {
  const o: any = literalBirth({ m: i }, "S" + i)
  keep.push(o)
  if (o._tag.literal !== "FilterS" + i || o._tag.pad.length !== 3) bad++
  const p = classBirth("S" + i)
  pairs.push(p)
  if (p.left.literal !== "LeftS" + i || p.left.pad.length !== 3) bad++
}
churn(20000)
for (let i = 0; i < keep.length; i++) {
  const o = keep[i]
  if (o._tag.literal !== "FilterS" + i || o._tag.pad[2] !== 3) bad++
  if (o.annotations.inner.a !== 1 || o.annotations.extra[1] !== 5 || o.meta.m !== i) bad++
  const p = pairs[i]
  if (p.left.literal !== "LeftS" + i || p.right.inner.a !== 1) bad++
}
console.log("literals=" + keep.length + " pairs=" + pairs.length + " bad=" + bad)
