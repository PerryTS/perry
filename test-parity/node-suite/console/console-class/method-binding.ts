const log = console.log;
const mathLog = Math.log;

console.log("console log equals Math.log:", log === mathLog);
console.log("math log:", mathLog(Math.E));
[1, 2, 3].forEach(log);
