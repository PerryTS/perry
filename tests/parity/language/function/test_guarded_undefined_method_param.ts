// Perry exposes this hook; ordinary Node does not. The forced-evacuation test
// uses it to move live state from inside the specialized loop without changing
// the parity fixture's observable output.
const collect = (globalThis as unknown as { gc?: () => void }).gc;

class Scanner {
  scan(values: number[], filter?: (value: number) => boolean): number {
    let sum = 0;
    for (let index = 0; index < values.length; index++) {
      const value = values[index]!;
      if (index === 1 && collect) collect();
      if (filter && !filter(value)) continue;
      sum += value;
    }
    return sum;
  }

  scanReassigned(values: number[], filter?: (value: number) => boolean): number {
    filter = (value) => value > 1;
    let sum = 0;
    for (const value of values) {
      if (filter && !filter(value)) continue;
      sum += value;
    }
    return sum;
  }

  scanCaptured(values: number[], filter?: (value: number) => boolean): number {
    const currentFilter = () => filter;
    let sum = 0;
    for (const value of values) {
      const current = currentFilter();
      if (current && !current(value)) continue;
      sum += value;
    }
    return sum;
  }

  scanDefault(
    values: number[],
    filter: (value: number) => boolean = (value) => value > 1,
  ): number {
    let sum = 0;
    for (const value of values) {
      if (filter && !filter(value)) continue;
      sum += value;
    }
    return sum;
  }
}

const values = [1, 2, 3];
const scanner = new Scanner();
let calls = 0;
const even = (value: number): boolean => {
  calls++;
  return value % 2 === 0;
};

console.log("omitted", scanner.scan(values), calls);
console.log("undefined", scanner.scan(values, undefined), calls);
console.log("function", scanner.scan(values, even), calls);
console.log("null", (scanner.scan as any).call(scanner, values, null), calls);
console.log("false", (scanner.scan as any).call(scanner, values, false), calls);
console.log("zero", (scanner.scan as any).call(scanner, values, 0), calls);
console.log("empty", (scanner.scan as any).call(scanner, values, ""), calls);

try {
  (scanner.scan as any).call(scanner, values, {});
  console.log("object", false);
} catch (error) {
  console.log("object", error instanceof TypeError);
}

console.log("reassigned", scanner.scanReassigned(values, undefined));
console.log("captured", scanner.scanCaptured(values, even), calls);
console.log("default", scanner.scanDefault(values, undefined));

const own = new Scanner() as any;
own.scan = () => 77;
console.log("own override", own.scan(values, undefined));

(Scanner.prototype as any).scan = () => 88;
console.log("prototype override", new Scanner().scan(values, undefined));
