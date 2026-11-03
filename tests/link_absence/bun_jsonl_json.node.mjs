const source = '{"answer":42}\n';
const values = source.trim().split("\n").map(line => JSON.parse(line));
console.log(values[0].answer);
