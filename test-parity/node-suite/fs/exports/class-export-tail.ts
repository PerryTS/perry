// #3711: node:fs exposes a class/constructor + newer-helper export tail
// (Dir, Dirent, Stats, the ReadStream/WriteStream constructors and their
// FileReadStream/FileWriteStream aliases, Utf8Stream, _toUnixTimestamp,
// mkdtempDisposableSync, openAsBlob). Lock in Node's observable export
// shape so the surface can't silently regress.
import * as fs from "node:fs";

console.log("Dir:", typeof fs.Dir);
console.log("Dirent:", typeof fs.Dirent);
console.log("Stats:", typeof fs.Stats);
console.log("ReadStream:", typeof fs.ReadStream);
console.log("WriteStream:", typeof fs.WriteStream);
console.log("FileReadStream:", typeof fs.FileReadStream);
console.log("FileWriteStream:", typeof fs.FileWriteStream);
console.log("Utf8Stream:", typeof fs.Utf8Stream);
console.log("_toUnixTimestamp:", typeof fs._toUnixTimestamp);
console.log("mkdtempDisposableSync:", typeof fs.mkdtempDisposableSync);
console.log("openAsBlob:", typeof fs.openAsBlob);
