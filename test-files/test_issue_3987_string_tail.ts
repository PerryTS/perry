function check(label: string, condition: boolean): void {
  if (!condition) {
    throw new Error(label);
  }
}

function checkEq(label: string, actual: string, expected: string): void {
  if (actual !== expected) {
    throw new Error(label + ": " + actual + " !== " + expected);
  }
}

checkEq("number exponent upper threshold", String(1000000000000000000000), "1e+21");
checkEq("number exponent lower threshold", String(0.0000001), "1e-7");
checkEq("array string coercion", String(new Array(1, 2, 3)), "1,2,3");

const s = "globglob";
check("string NaN index", (s as any)[NaN] === undefined);
check("string Infinity index", (s as any)[Infinity] === undefined);
check("string negative index", (s as any)[-1] === undefined);
check("string fractional index", (s as any)[1.5] === undefined);
check("string out-of-range index", (s as any)[99] === undefined);
check("string canonical numeric index", (s as any)[0] === "g");
check("string string numeric index", (s as any)["1"] === "l");
check("string noncanonical string index", (s as any)["01"] === undefined);

let sideEffects = 0;
function extra(): number {
  sideEffects += 1;
  return 99;
}

check("charAt ignores extra args", s.charAt(0, extra(), extra(), extra()) === "g");
check("charCodeAt ignores extra args", s.charCodeAt(0, extra(), extra(), extra()) === 103);
check("ignored extra args are evaluated", sideEffects === 6);

console.log("string-tail-3987 ok");
