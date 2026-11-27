// #15494: a nested function declaration lowers its declared parameter and
// return types, so codegen may pick typed paths for it. An erased annotation
// is not a proof of the runtime value: every `lie:` row passes a value the
// annotation denies, and must behave as the untyped JavaScript does. The
// `honest:` rows are the typed paths the annotations exist to enable.

const str: any = "7";
const num: any = 7;
const arr: any = [1, 2, 3];
const obj: any = { length: 5, valueOf: () => 40 };

function run() {
  function add(a: number, b: number): number {
    return a + b;
  }
  function len(s: string): number {
    return s.length;
  }
  function not(flag: boolean): boolean {
    return !flag;
  }
  function head(xs: number[]): number {
    return xs[0];
  }
  function half(n: number): number {
    return n / 2;
  }
  function trunc(n: number): number {
    return n | 0;
  }
  function claimsNumber(): number {
    return str;
  }

  // ---- honest ----
  console.log("honest add", add(2, 3));
  console.log("honest len", len("abcd"));
  console.log("honest not", not(false));
  console.log("honest head", head([9, 8]));
  console.log("honest trunc", trunc(7.9));
  let sum = 0;
  for (let i = 0; i < 1000; i++) sum = add(sum, i);
  console.log("honest loop", sum);

  // ---- lie: number params ----
  console.log("lie add string", add(str, 1), typeof add(str, 1));
  console.log("lie add both strings", add(str, str));
  console.log("lie add undefined", add(num, undefined as any));
  console.log("lie add object", add(obj, 2));
  console.log("lie half string", half(str));
  console.log("lie half array", half(arr));
  console.log("lie trunc string", trunc("12.7" as any));

  // ---- lie: string, boolean and array params ----
  console.log("lie len array", len(arr));
  console.log("lie len object", len(obj));
  console.log("lie len number", len(num));
  console.log("lie not number", not(0 as any), not(num));
  console.log("lie not string", not("" as any), not(str));
  console.log("lie head string", head(str));
  console.log("lie head object", head(obj));

  // ---- lie: the declared return type ----
  const r = claimsNumber();
  console.log("lie return", r + 1, typeof r, r.length);

  // ---- lie: the same call site sees honest and lying arguments ----
  const mixed: any[] = [1, "1", null, [2], true];
  for (const m of mixed) console.log("mixed", add(m, 1));
}
run();
