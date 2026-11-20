// Dense all-numeric-literal switches (>= 8 unique integer values) lower to a
// binary-search dispatch tree instead of the linear js_switch_strict_equals
// tower. Only the DISPATCH changes; this battery pins every observable
// semantic the tree must preserve:
//   - first / middle / last case selection in a 40-case switch
//   - default clause (including a default placed mid-block, with fall-through
//     out of it)
//   - NaN discriminant matches nothing -> default
//   - fall-through across 3 consecutive case bodies
//   - duplicate case value -> FIRST clause wins (source order)
//   - negative case values and negative discriminants
//   - non-integer discriminant missing every case -> default
//   - non-number discriminants (string/bool/null/undefined/object) -> default
//   - int32-boxed vs double-boxed numeric discriminants agree
//   - a hot loop summing dispatch results

// 40 case clauses (39 unique values + one duplicate). Values span negatives,
// -0, int32 limits, and beyond-int32 doubles.
function label(x: any): string {
  switch (x) {
    case 0: return "zero"; // first case
    case 1: return "one";
    case 2: return "two";
    case 3: return "three";
    case 4: return "four";
    case 5: return "five";
    case 6: return "six";
    case 7: return "seven-first"; // duplicate value: this one must win
    case 7: return "seven-second";
    case 8: return "eight";
    case 9: return "nine";
    case 10: return "ten";
    case 11: return "eleven";
    case 12: return "twelve";
    case 13: return "thirteen";
    case 14: return "fourteen";
    case 15: return "fifteen";
    case 16: return "sixteen";
    case 17: return "seventeen";
    case 18: return "eighteen";
    case 19: return "nineteen"; // middle-ish case
    case 20: return "twenty";
    case 25: return "twenty-five";
    case 30: return "thirty";
    case 31: return "thirty-one";
    case 32: return "thirty-two";
    case 33: return "thirty-three";
    case 64: return "sixty-four";
    case 512: return "five-twelve";
    case 999: return "nine-nine-nine";
    case 100000: return "hundred-k";
    case 1000000: return "million";
    case 2147483647: return "int32-max";
    case 2147483648: return "beyond-int32";
    case -1: return "neg-one";
    case -2: return "neg-two";
    case -5: return "neg-five";
    case -100: return "neg-hundred";
    case -2147483648: return "int32-min";
    case -0: return "neg-zero-clause"; // 0 === -0: 'case 0' above wins (last case clause)
    default: return "default:" + String(x);
  }
}

// first / middle / last / default
console.log(label(0));
console.log(label(19));
console.log(label(-0)); // -0 === 0 -> "zero", not the -0 clause
console.log(label(-2147483648));
console.log(label(2147483648));
console.log(label(7)); // duplicate value -> first clause
console.log(label(-100));
console.log(label(34)); // between case values -> default
console.log(label(-3)); // negative miss -> default
console.log(label(NaN)); // NaN matches nothing -> default
console.log(label(2.5)); // non-integer discriminant -> default
console.log(label(-0.5));
console.log(label(1e300)); // way above every case -> default
console.log(label(-1e300)); // way below every case -> default
console.log(label("7")); // string never matches a number case
console.log(label(true));
console.log(label(null));
console.log(label(undefined));
console.log(label({}));

// int32-boxed vs double-boxed discriminants must dispatch identically.
const arr: number[] = [];
for (let i = 0; i < 40; i++) arr.push(i);
console.log(label(arr[31])); // runtime int32-ish value
console.log(label(arr[31] + 0.0)); // arithmetic result
console.log(label(62 / 2)); // division result (double lane)
console.log(label(1024 * 1024 * 2048)); // 2^31 computed at runtime

// Fall-through across 3 consecutive case bodies, in a tree-qualifying switch.
function fall(x: number): string {
  let s = "";
  switch (x) {
    case 10:
      s += "a";
    case 11:
      s += "b";
    case 12:
      s += "c";
      break;
    case 13:
      s += "d";
      break;
    case 14:
      s += "e";
    case 15:
      s += "f";
      break;
    case 16:
      s += "g";
      break;
    case 17:
      s += "h";
      break;
  }
  return "[" + s + "]";
}
console.log(fall(10)); // abc
console.log(fall(11)); // bc
console.log(fall(12)); // c
console.log(fall(14)); // ef
console.log(fall(17)); // h
console.log(fall(99)); // (empty, no default)

// Default placed mid-block: cases after it are still tested first, and its
// body falls through into the next case body.
function midDefault(x: any): string {
  let s = "";
  switch (x) {
    case 1:
      s += "1";
      break;
    case 2:
      s += "2"; // falls into default body
    default:
      s += "D"; // falls into case 3 body
    case 3:
      s += "3";
      break;
    case 4:
      s += "4";
      break;
    case 5:
      s += "5";
      break;
    case 6:
      s += "6";
      break;
    case 7:
      s += "7";
      break;
    case 8:
      s += "8";
      break;
    case 9:
      s += "9";
      break;
  }
  return "<" + s + ">";
}
console.log(midDefault(2)); // <2D3>
console.log(midDefault(3)); // <3> (default skipped: 3 matched)
console.log(midDefault(9)); // <9>
console.log(midDefault(42)); // <D3>
console.log(midDefault(NaN)); // <D3>
console.log(midDefault("3")); // <D3>

// A non-integer literal among the cases keeps the tower lowering; both
// lowerings must agree on these.
function halfStep(x: number): string {
  switch (x) {
    case 0.5: return "half";
    case 1: return "t-one";
    case 2: return "t-two";
    case 3: return "t-three";
    case 4: return "t-four";
    case 5: return "t-five";
    case 6: return "t-six";
    case 7: return "t-seven";
    case 8: return "t-eight";
    default: return "t-default";
  }
}
console.log(halfStep(0.5));
console.log(halfStep(8));
console.log(halfStep(0.25));

// Hot loop: 64-way dispatch summed 200k times, negatives included.
function hot(): number {
  let sum = 0;
  for (let i = 0; i < 200000; i++) {
    const k = (i % 64) - 8;
    switch (k) {
      case -8: sum += 1; break;
      case -7: sum += 2; break;
      case -5: sum += 3; break;
      case -1: sum += 4; break;
      case 0: sum += 5; break;
      case 1: sum += 6; break;
      case 2: sum += 7; break;
      case 3: sum += 8;
      case 4: sum += 9; break; // fall-through pair inside the hot switch
      case 7: sum += 10; break;
      case 11: sum += 11; break;
      case 19: sum += 12; break;
      case 23: sum += 13; break;
      case 31: sum += 14; break;
      case 42: sum += 15; break;
      case 55: sum += 16; break;
      default: sum -= 1; break;
    }
  }
  return sum;
}
console.log(hot());
