// Object.assign stores into Math (20.1.2.1 Set on the target).
const x = Number(process.argv[2] ?? "0");
Object.assign(Math, { cos: (_v: number) => 17 });
console.log(Math.cos(x));
