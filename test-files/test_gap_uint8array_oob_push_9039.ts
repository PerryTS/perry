const u = new Uint8Array([7, 8, 9]);
const a: number[] = [];

for (let i = 0; i < 3; i++) {
  const x = u[99];
  a.push(x);
}

console.log(JSON.stringify(a), u[99], typeof u[99]);
