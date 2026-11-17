// Math escapes through an alias into a writer.
function put(o: any, key: string): void { o[key] = (_v: number) => 11; }
const M = Math;
put(M, process.argv[2] ?? "abs");
console.log(Math.abs(-2));
