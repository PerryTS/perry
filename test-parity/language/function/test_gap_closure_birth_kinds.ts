function kinds(n: number) {
  return [
    function ordinary() { return n; },
    async function asynchronous() { return n; },
    function* generator() { yield n; },
    async function* asyncGenerator() { yield n; },
  ];
}
const refs = [function () {}, async function () {}, function* () {}, async function* () {}];
const values = kinds(17);
for (let i = 0; i < values.length; i++) {
  console.log(values[i].name, values[i].length,
    Object.getPrototypeOf(values[i]) === Object.getPrototypeOf(refs[i]));
}
