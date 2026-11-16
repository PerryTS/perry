// A computed-key store whose key is unknown at compile time.
const k = process.argv[2] ?? "sin";
(Math as any)[k] = (_v: number) => 99;
console.log(Math.sin(1));
