// #10190: numeric keys that are not array indices are named properties.
function read(receiver: any, key: any): any { return receiver[key]; }
function numeric(receiver: any, key: number): any { return receiver[key]; }
function typed(receiver: number[], key: number): any { return receiver[key]; }

const rows: any = JSON.parse('[{"id":1},{"id":2},{"id":3}]');
console.log("repro", rows[0.5], rows[-0.5]);
try {
  console.log(rows[0.5].id);
} catch (error) {
  console.log("repro throws", error instanceof TypeError);
}

const keys = [0.5, -0.5, 1.5, -1, NaN, Infinity, -Infinity,
  2147483648, 4294967295, 0, -0, 1, 2, 3];
for (const array of [[11, 22, 33], JSON.parse('[11,22,33]')]) {
  for (const key of keys) {
    console.log("absent", key, read(array, key), numeric(array, key), typed(array, key));
  }
  for (let i = 0; i < 9; i++) {
    const key = keys[i];
    Object.defineProperty(array, String(key), { value: 100 + i, configurable: true });
    console.log("own", key, read(array, key), numeric(array, key), typed(array, key));
  }
  console.log("elements", array.length, array[0], array[1], array[2]);
}

const inherited: any = [11, 22];
const prototype = Object.create(Array.prototype);
Object.defineProperty(prototype, "0.5", { get() { return this[1] + 7; } });
Object.setPrototypeOf(inherited, prototype);
console.log("inherited", read(inherited, 0.5), numeric(inherited, 0.5));
