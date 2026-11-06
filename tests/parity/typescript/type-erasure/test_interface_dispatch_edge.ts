// Test interface method dispatch edge cases
// Critical for static typing migration: interface dispatch must work with different implementors

interface Formatter {
  format(value: string): string;
}

class UpperFormatter implements Formatter {
  format(value: string): string {
    return value.toUpperCase();
  }
}

class BracketFormatter implements Formatter {
  format(value: string): string {
    return "[" + value + "]";
  }
}

class PrefixFormatter implements Formatter {
  prefix: string;
  constructor(prefix: string) {
    this.prefix = prefix;
  }
  format(value: string): string {
    return this.prefix + ": " + value;
  }
}

// === Basic interface dispatch ===
function applyFormat(f: Formatter, text: string): string {
  return f.format(text);
}

const upper = new UpperFormatter();
const bracket = new BracketFormatter();
const prefix = new PrefixFormatter("LOG");

console.log(applyFormat(upper, "hello"));    // HELLO
console.log(applyFormat(bracket, "hello"));  // [hello]
console.log(applyFormat(prefix, "hello"));   // LOG: hello

// === Interface variable reassignment ===
let fmt: Formatter = upper;
console.log(fmt.format("test")); // TEST
fmt = bracket;
console.log(fmt.format("test")); // [test]
fmt = prefix;
console.log(fmt.format("test")); // LOG: test

// === Array of interface-typed values ===
const formatters: Formatter[] = [upper, bracket, prefix];
for (let i = 0; i < formatters.length; i++) {
  console.log(formatters[i].format("hi"));
}
// HI
// [hi]
// LOG: hi

// === Interface in class field ===
class Pipeline {
  formatter: Formatter;
  constructor(f: Formatter) {
    this.formatter = f;
  }
  process(input: string): string {
    return this.formatter.format(input);
  }
}

const p1 = new Pipeline(upper);
console.log(p1.process("world")); // WORLD

const p2 = new Pipeline(bracket);
console.log(p2.process("world")); // [world]

// === Multiple methods in interface ===
interface Validator {
  validate(input: string): boolean;
  getMessage(): string;
}

class LengthValidator implements Validator {
  minLen: number;
  constructor(min: number) { this.minLen = min; }
  validate(input: string): boolean {
    return input.length >= this.minLen;
  }
  getMessage(): string {
    return "Must be at least " + this.minLen + " chars";
  }
}

class NonEmptyValidator implements Validator {
  validate(input: string): boolean {
    return input.length > 0;
  }
  getMessage(): string {
    return "Must not be empty";
  }
}

function check(v: Validator, input: string): string {
  if (v.validate(input)) {
    return "OK";
  }
  return "FAIL: " + v.getMessage();
}

const lenV = new LengthValidator(5);
const neV = new NonEmptyValidator();

console.log(check(lenV, "hello"));   // OK
console.log(check(lenV, "hi"));      // FAIL: Must be at least 5 chars
console.log(check(neV, "x"));        // OK
console.log(check(neV, ""));         // FAIL: Must not be empty
