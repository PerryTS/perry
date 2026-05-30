import * as buffer from "node:buffer";

const BlobCtor = buffer.Blob;
console.log("BlobCtor typeof:", typeof BlobCtor);
const blob = new BlobCtor(["hello"]);
console.log("Blob size:", blob.size);
console.log("Blob text:", await blob.text());

const FileCtor = buffer.File;
console.log("FileCtor typeof:", typeof FileCtor);
const file = new FileCtor(["hello"], "greeting.txt", {
  lastModified: 1700000000000,
});
console.log("File name:", file.name);
console.log("File lastModified:", file.lastModified);
console.log("File size:", file.size);
console.log("File text:", await file.text());
