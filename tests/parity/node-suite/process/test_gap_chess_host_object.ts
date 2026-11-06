// console and process.stdout are runtime-born host objects: their members are the runtime natives unless the program writes them.
console.log("ok");
process.stdout.write("line\n");
process.stderr.write("");
