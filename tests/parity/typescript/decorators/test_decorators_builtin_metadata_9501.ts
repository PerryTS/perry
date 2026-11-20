// #9501: TypeScript's legacy decorator metadata stores the global constructor
// value for built-in types, not Perry's internal class-reference sentinel.
// Expected output was generated with TypeScript 5.9.3 and reflect-metadata
// 0.2.2 using --experimentalDecorators --emitDecoratorMetadata.
import "reflect-metadata";

function Decorated(): any {
  return () => {};
}

class UserType {}

class MetadataTypes {
  @Decorated() numberValue!: number;
  @Decorated() stringValue!: string;
  @Decorated() booleanValue!: boolean;
  @Decorated() objectValue!: object;
  @Decorated() arrayValue!: number[];
  @Decorated() tupleValue!: [number, string];
  @Decorated() functionValue!: () => void;
  @Decorated() symbolValue!: symbol;
  @Decorated() bigintValue!: bigint;
  @Decorated() promiseValue!: Promise<string>;
  @Decorated() unionValue!: number | string;
  @Decorated() anyValue!: any;
  @Decorated() unknownValue!: unknown;
  @Decorated() userValue!: UserType;

  @Decorated()
  method(
    numberValue: number,
    stringValue: string,
    booleanValue: boolean,
    objectValue: object,
    arrayValue: number[],
    tupleValue: [number, string],
    functionValue: () => void,
    symbolValue: symbol,
    bigintValue: bigint,
    promiseValue: Promise<string>,
    unionValue: number | string,
    anyValue: any,
    unknownValue: unknown,
    userValue: UserType,
  ) {}
}

const expected: Array<[string, any]> = [
  ["numberValue", Number],
  ["stringValue", String],
  ["booleanValue", Boolean],
  ["objectValue", Object],
  ["arrayValue", Array],
  ["tupleValue", Array],
  ["functionValue", Function],
  ["symbolValue", Symbol],
  ["bigintValue", BigInt],
  ["promiseValue", Promise],
  ["unionValue", Object],
  ["anyValue", Object],
  ["unknownValue", Object],
  ["userValue", UserType],
];

for (const [property, constructor] of expected) {
  const actual = Reflect.getMetadata("design:type", MetadataTypes.prototype, property);
  console.log(property, typeof actual, actual === constructor, actual && actual.name);
}

const params = Reflect.getMetadata("design:paramtypes", MetadataTypes.prototype, "method");
console.log("params length", params.length);
console.log(
  "params identities",
  params.every((actual: any, index: number) => actual === expected[index][1]),
);

const numberType = Reflect.getMetadata("design:type", MetadataTypes.prototype, "numberValue");
console.log(
  "new via number metadata",
  typeof numberType === "function" ? new numberType(5).valueOf() : "not a constructor",
);
