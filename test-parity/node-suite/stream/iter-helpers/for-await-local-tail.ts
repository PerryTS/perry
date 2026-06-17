import * as stream from "node:stream";

const flatMapSource = stream.Readable.from([1, 2]);
const flatMapOut: number[] = [];
for await (const value of flatMapSource.flatMap((n: number) => [n, n * 10])) {
  flatMapOut.push(value as number);
}
console.log("flatMap local:", flatMapOut.join(","));

const dropSource = stream.Readable.from([1, 2, 3, 4]);
const dropOut: number[] = [];
for await (const value of dropSource.drop(2)) {
  dropOut.push(value as number);
}
console.log("drop local:", dropOut.join(","));

const takeSource = stream.Readable.from([1, 2, 3, 4]);
const takeOut: number[] = [];
for await (const value of takeSource.take(2)) {
  takeOut.push(value as number);
}
console.log("take local:", takeOut.join(","));
