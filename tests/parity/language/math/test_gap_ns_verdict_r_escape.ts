// Math escapes into a function that writes through its parameter.
function put(o: any, key: string): void { o[key] = (_v: number) => 7; }
put(Math, process.argv[2] ?? "sqrt");
console.log(Math.sqrt(16));
