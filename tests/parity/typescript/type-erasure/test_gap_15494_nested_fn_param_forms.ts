// #15494: the declared types of a nested function declaration now reach its
// parameters, so every parameter form must still bind the value it did when
// the parameters were untyped: defaults, rest, destructuring, hoisted calls,
// recursion, async and generator bodies.

const lie: any = "not a number";

function run() {
  console.log("hoisted", early(4));
  function early(n: number): number {
    return n * 2;
  }

  function withDefault(n: number = 10, label: string = "d"): string {
    return label + n;
  }
  console.log("default", withDefault(), withDefault(1), withDefault(undefined, "x"));
  console.log("default lie", withDefault(lie, lie));

  function total(...xs: number[]): number {
    let s = 0;
    for (const x of xs) s += x;
    return s;
  }
  console.log("rest", total(), total(1, 2, 3), total(1, lie));

  function point({ x, y }: { x: number; y: number }, [z]: number[]): number {
    return x + y + z;
  }
  console.log("destructure", point({ x: 1, y: 2 }, [3]), point({ x: lie, y: 2 }, [3]));

  function fact(n: number): number {
    return n <= 1 ? 1 : n * fact(n - 1);
  }
  console.log("recursion", fact(10), fact("4" as any));

  async function later(n: number): Promise<number> {
    return n + 1;
  }
  later(1).then((v) => console.log("async", v));
  later(lie).then((v) => console.log("async lie", v));

  function* count(limit: number): Generator<number> {
    for (let i = 0; i < limit; i++) yield i;
  }
  console.log("generator", [...count(3)], [...count("2" as any)]);
}
run();
