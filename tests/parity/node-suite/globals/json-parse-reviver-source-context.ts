function own(obj: unknown, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

function kind(value: unknown): string {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

function sourceInfo(context: any): string {
  if (context === undefined) return "context:undefined";
  const keys = Object.keys(context).join(",");
  if (!own(context, "source")) {
    return "empty:" + keys;
  }
  const desc = Object.getOwnPropertyDescriptor(context, "source");
  const flags = `${desc?.writable}/${desc?.enumerable}/${desc?.configurable}`;
  return `source:${String(context.source)}:${keys}:${flags}`;
}

const sourceRows: string[] = [];
const parsed = JSON.parse(
  '{"s":"a\\\\nb","n":1.1e+1,"b":true,"z":null,"o":{"x":2},"a":[3],"dup":1,"dup":2}',
  function (key, value, context) {
    sourceRows.push(`${key}:${kind(value)}:${sourceInfo(context)}`);
    return value;
  },
);
console.log("sources:", sourceRows.join("|"));
console.log("result:", JSON.stringify(parsed));

const forwardRows: string[] = [];
const forward = JSON.parse('{"a":1,"b":2,"c":3,"d":{"x":4},"e":5}', function (
  key,
  value,
  context,
) {
  if (key === "a") {
    this.b = 9;
    this.c = 3;
    this.d = 5;
    this.e = { y: 6 };
  }
  if (key !== "") {
    forwardRows.push(`${key}:${kind(value)}:${String(value)}:${sourceInfo(context)}`);
  }
  return value;
});
console.log("forward:", forwardRows.join("|"));
console.log("forward result:", JSON.stringify(forward));

const replaceObjectRows: string[] = [];
const replacedObject = JSON.parse('{"a":1,"b":{"x":2}}', function (key, value, context) {
  if (key === "a") {
    this.b = { x: 2 };
  }
  if (key !== "") {
    replaceObjectRows.push(`${key}:${kind(value)}:${String(value)}:${sourceInfo(context)}`);
  }
  return value;
});
console.log("replace object:", replaceObjectRows.join("|"));
console.log("replace object result:", JSON.stringify(replacedObject));

const deleteRows: string[] = [];
const deleted = JSON.parse('{"a":1,"b":2}', function (key, value, context) {
  if (key === "a") {
    delete this.b;
  }
  if (key !== "") {
    deleteRows.push(`${key}:${kind(value)}:${String(value)}:${sourceInfo(context)}`);
  }
  return value;
});
console.log("delete:", deleteRows.join("|"), JSON.stringify(deleted), own(deleted, "b"));

let rootRow = "";
const rootPrimitive = JSON.parse("9007199254740993", function (key, value, context) {
  rootRow = `${key}:${kind(value)}:${String(value)}:${sourceInfo(context)}`;
  return context.source;
});
console.log("root primitive:", rootPrimitive, rootRow);
