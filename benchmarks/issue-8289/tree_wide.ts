class Tree {
  left: Tree | null;
  right: Tree | null;
  a: number; b: number; c: number; d: number;
  e: number; f: number; g: number; h: number;
  constructor(left: Tree | null, right: Tree | null, s: number) {
    this.left = left;
    this.right = right;
    this.a = s; this.b = s + 1; this.c = s + 2; this.d = s + 3;
    this.e = s + 4; this.f = s + 5; this.g = s + 6; this.h = s + 7;
  }
}

function build(depth: number, s: number): Tree {
  if (depth === 0) return new Tree(null, null, s);
  return new Tree(build(depth - 1, s), build(depth - 1, s + 1), s);
}

function count(t: Tree): number {
  if (t.left === null) return 1;
  return 1 + count(t.left) + count(t.right as Tree);
}

function main(): void {
  let total = 0;
  for (let i = 0; i < 40; i++) {
    const t = build(18, i);
    total += count(t);
  }
  console.log(total);
}
main();
