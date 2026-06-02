// process.umask() reads the process file-mode creation mask (a number).
console.log("is number:", typeof process.umask() === "number");
