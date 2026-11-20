import * as stream from "node:stream";

const forEachOut: string[] = [];
await stream.Readable.from(["a", "b"]).forEach((value: string) => {
  forEachOut.push(value);
});
console.log("forEach:", forEachOut.join(","));

const flatMapOut: number[] = [];
for await (const value of stream.Readable.from([1, 2]).flatMap((n: number) => [n, n * 10])) {
  flatMapOut.push(value as number);
}
console.log("flatMap:", flatMapOut.join(","));

const dropOut: number[] = [];
for await (const value of stream.Readable.from([1, 2, 3, 4]).drop(2)) {
  dropOut.push(value as number);
}
console.log("drop:", dropOut.join(","));

const takeOut: number[] = [];
for await (const value of stream.Readable.from([1, 2, 3, 4]).take(2)) {
  takeOut.push(value as number);
}
console.log("take:", takeOut.join(","));

const reduced = await stream.Readable.from([1, 2, 3, 4]).reduce(
  (acc: number, value: number) => acc + value,
  0,
);
console.log("reduce:", reduced);
