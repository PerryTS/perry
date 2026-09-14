// A positive then lookup; the existing negative-then probe cannot serve it.
const count = Number(process.argv[2] || "20000");
const thenable: any = { then(resolve: any) { resolve(7); } };
let sum = 0;
let completed = 0;
for (let i = 0; i < count; i++) {
  Promise.resolve(thenable).then((value: number) => {
    sum += value;
    if (++completed === count) {
      if (sum !== count * 7) throw new Error("thenable mismatch");
      console.log(sum);
    }
  });
}
