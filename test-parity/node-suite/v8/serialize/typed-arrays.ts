import { deserialize, serialize } from "node:v8";

for (
  const value of [
    new Int8Array([-1, 2]),
    new Uint8Array([0, 255]),
    new Uint8ClampedArray([-1, 300]),
    new Int16Array([-2, 300]),
    new Uint16Array([2, 65535]),
    new Int32Array([-3, 70000]),
    new Uint32Array([3, 4000000000]),
    new Float32Array([1.5, -2.25]),
    new Float64Array([Math.PI, -0]),
    new BigInt64Array([-1n, 2n]),
    new BigUint64Array([1n, 2n]),
  ] as const
) {
  const result: any = deserialize(serialize(value));
  const values = [...result].map((item: any) =>
    typeof item === "bigint"
      ? item + "n"
      : Object.is(item, -0)
      ? "-0"
      : String(item)
  );
  console.log(
    value.constructor.name + ":",
    result.constructor.name,
    result.byteLength,
    values.join(","),
  );
}
