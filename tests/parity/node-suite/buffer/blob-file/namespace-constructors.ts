import * as buffer from "node:buffer";

const BlobCtor = buffer.Blob;
const blob = new BlobCtor(["hello"]);
console.log("namespace Blob ctor same:", BlobCtor === buffer.Blob, BlobCtor === globalThis.Blob);
console.log("namespace Blob size:", blob.size);
console.log("namespace Blob text:", await blob.text());
console.log("namespace Blob direct:", await new buffer.Blob(["direct"]).text());

const FileCtor = buffer.File;
const file = new FileCtor(["hello"], "greeting.txt", {
  type: "text/plain",
  lastModified: 1700000000000,
});
console.log("namespace File ctor same:", FileCtor === buffer.File, FileCtor === globalThis.File);
console.log("namespace File fields:", file.name, file.type, file.size, file.lastModified);
console.log("namespace File text:", await file.text());
const directFile = new buffer.File(["direct"], "direct.txt");
console.log("namespace File direct:", directFile.name, await directFile.text());
