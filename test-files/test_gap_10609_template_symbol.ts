// Template substitutions use implicit ToString: Symbol values must throw,
// while the explicit String constructor is allowed to describe a Symbol.
const symbol = Symbol("template");

function report(label: string, operation: () => unknown): void {
  try {
    console.log(label, operation());
  } catch (error: any) {
    console.log(label, error.constructor.name, error.message);
  }
}

report("single", () => `${symbol}`);
report("multiple", () => `before ${symbol} after`);
const declaredNumber: number = symbol as any;
report("declared number", () => `before ${declaredNumber} after`);
const primitiveObject = { [Symbol.toPrimitive](): symbol { return symbol; } };
report("object primitive", () => `${primitiveObject}`);
const stringObject = { toString(): symbol { return symbol; } };
report("object toString", () => `${stringObject}`);
report("String", () => String(symbol));
report("method", () => symbol.toString());
