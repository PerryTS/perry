// BigInt mixed-operand matrix (#6649 neighbors): every arithmetic / bitwise
// operator with bigint×number / string / boolean / null / undefined operands,
// in both orders, matching Node's exact TypeError messages where Node throws
// and Node's values where the operation is defined (`+` with a string operand
// concatenates; relational and equality comparisons never throw).
//
// All operands flow through an `any[]`, so the compiled code takes the
// dynamic-dispatch paths (the same ones the pi bundle's TypeBox/zod/json-bigint
// init code exercises) rather than statically-typed fast paths.
function probe(label: string, fn: () => unknown): void {
  try {
    console.log(label, "ok", String(fn()));
  } catch (e) {
    const err = e as Error;
    console.log(label, "throw", err.name, err.message);
  }
}

const vals: any[] = [1n, 2, "3", true, null, undefined, 3n];
const big: any = vals[0];
const num: any = vals[1];
const str: any = vals[2];
const bool: any = vals[3];
const nil: any = vals[4];
const undef: any = vals[5];
const big2: any = vals[6];

// + : string operand concatenates; any other non-bigint operand throws.
probe("add big num", () => big + num);
probe("add num big", () => num + big);
probe("add big str", () => big + str);
probe("add str big", () => str + big);
probe("add big bool", () => big + bool);
probe("add big null", () => big + nil);
probe("add big undef", () => big + undef);
probe("add big big", () => big + big2);

// Arithmetic operators: mixed always throws, both-bigint computes.
probe("sub big num", () => big - num);
probe("sub num big", () => num - big);
probe("sub big big", () => big - big2);
probe("mul big num", () => big * num);
probe("mul num big", () => num * big);
probe("mul big str", () => big * str);
probe("mul big bool", () => big * bool);
probe("mul bool big", () => bool * big);
probe("mul big big", () => big * big2);
probe("div big num", () => big / num);
probe("div big big", () => big / big2);
probe("mod big num", () => big % num);
probe("mod big big", () => big % big2);
probe("pow big num", () => big ** num);
probe("pow big big", () => big2 ** big);

// Bitwise / shifts: mixed throws; >>> has its own both-bigint error.
probe("and big num", () => big & num);
probe("and big big", () => big & big2);
probe("or big num", () => big | num);
probe("or big big", () => big | big2);
probe("xor big num", () => big ^ num);
probe("xor big big", () => big ^ big2);
probe("shl big num", () => big << num);
probe("shl big big", () => big << big2);
probe("shr big num", () => big >> num);
probe("shr big big", () => big2 >> big);
probe("ushr big num", () => big >>> num);
probe("ushr num big", () => num >>> big);
probe("ushr big str", () => big >>> str);
probe("ushr big big", () => big >>> big2);

// Relational / equality: never throw for bigint×number/string.
probe("lt big num", () => big < num);
probe("gt num big", () => num > big);
probe("le big str", () => big <= str);
probe("ge str big", () => str >= big);
probe("looseeq big num", () => big == 1);
probe("looseeq big str", () => big == "1");
probe("stricteq big num", () => big === (1 as any));
probe("ne big num", () => big != num);

// The TypeBox FNV-1a shape: accumulator loop over ^, *, % with all-bigint
// operands stays on the bigint path (no throw, node-identical value).
let acc: any = BigInt("14695981039346656037");
const prime: any = BigInt("1099511628211");
const size: any = BigInt("18446744073709551616");
acc = acc ^ BigInt(8);
acc = acc * prime % size;
console.log("fnv-step:", String(acc));
