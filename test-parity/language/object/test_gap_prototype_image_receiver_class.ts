const P: any = { x: "p", pad: "p-pad" };
const Q: any = { pad: "q-pad", x: "q" };
const p: any = Object.create(P);
p.y = 1;
const q: any = Object.create(Q);
q.y = 1;
for (let i = 0; i < 2; i++) {
  const holder: any = i === 0 ? P : Q;
  console.log(holder.x);
}
