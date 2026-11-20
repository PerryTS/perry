import * as fs from "node:fs";

console.log("F_OK zero:", fs.constants.F_OK === 0);
console.log("R_OK number:", typeof fs.constants.R_OK === "number");
console.log("W_OK number:", typeof fs.constants.W_OK === "number");
console.log("COPYFILE_EXCL number:", typeof fs.constants.COPYFILE_EXCL === "number");
console.log("O_RDONLY number:", typeof fs.constants.O_RDONLY === "number");

console.log("O_CREAT number:", typeof fs.constants.O_CREAT === "number");
console.log("O_TRUNC number:", typeof fs.constants.O_TRUNC === "number");
console.log("O_APPEND number:", typeof fs.constants.O_APPEND === "number");
console.log("O_EXCL number:", typeof fs.constants.O_EXCL === "number");
console.log("COPYFILE_FICLONE number:", typeof fs.constants.COPYFILE_FICLONE === "number");
console.log("COPYFILE_FICLONE_FORCE number:", typeof fs.constants.COPYFILE_FICLONE_FORCE === "number");
console.log("S_IRUSR octal:", fs.constants.S_IRUSR.toString(8));
console.log("S_IWUSR octal:", fs.constants.S_IWUSR.toString(8));
console.log("S_IXUSR octal:", fs.constants.S_IXUSR.toString(8));
console.log("S_IROTH octal:", fs.constants.S_IROTH.toString(8));
