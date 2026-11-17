// Object.defineProperty itself replaced.
const k = process.argv[2] ?? "a";
(Object as any).defineProperty = (_o: object, p: string) => "fake:" + p;
console.log(Object.defineProperty({}, k, { value: 1 }));
