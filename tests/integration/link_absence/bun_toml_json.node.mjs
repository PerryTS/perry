const source = "answer = 42";
const [name, value] = source.split("=");
console.log({[name.trim()]: Number(value.trim())}.answer);
