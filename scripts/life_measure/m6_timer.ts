// m6: captured by a setTimeout closure: alive at the end of the birth's turn.
function arm(i: number): void {
  const o = { a: i, b: "t" };
  setTimeout(() => {
    console.log(o.a);
  }, 0);
}
for (let i = 0; i < 20; i++) arm(i);
console.log("armed");
