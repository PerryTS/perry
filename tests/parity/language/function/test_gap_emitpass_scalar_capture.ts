// A numeric loop condition cannot prove a captured value scalar after a call.
function storeChanged(target: any) {
  let value: any = 0;
  function receiver() {
    value = { marker: "retained pointer" };
    return target;
  }
  while (value < 1) {
    receiver().child = value;
    break;
  }
}
function churn() {
  const garbage: any[] = [];
  for (let i = 0; i < 120000; i++) {
    garbage.push({ text: "transient heap string " + i });
  }
  return garbage.length;
}
const parent: any = { child: null };
storeChanged(parent);
console.log(churn());
storeChanged(parent);
console.log(churn());
console.log(parent.child.marker);
