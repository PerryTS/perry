// Reflection census must survive the immediate process.exit path too.
function caféExit() { return "世界😀"; }
console.log(Function.prototype.toString.call(caféExit));
process.exit(0);
