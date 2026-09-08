// Regression for #9842: FormData.append/set must preserve Blob and File
// values, and Request must serialize FormData as a non-empty multipart body.

const form = new FormData();
const original = new File(
  [new Uint8Array([0, 255, 13, 10, 65])],
  "original.bin",
  { type: "application/octet-stream", lastModified: 1234 },
);
form.append("caption", "Perry upload");
form.append("original", original);
form.append("renamed", original, "renamed.bin");
form.set("blob", new Blob(["blob payload"], { type: "text/plain" }));

const originalEntry = form.get("original") as File;
const renamedEntry = form.get("renamed") as File;
const blobEntry = form.get("blob") as File;
console.log(
  `entries=${originalEntry instanceof File}/${originalEntry.name}/${originalEntry.lastModified};` +
    `${renamedEntry instanceof File}/${renamedEntry.name};` +
    `${blobEntry instanceof File}/${blobEntry.name}/${blobEntry.type}`,
);

const request = new Request("https://example.test/upload", {
  method: "POST",
  body: form,
});
const contentType = request.headers.get("content-type") || "";
console.log(`multipart=${contentType.startsWith("multipart/form-data; boundary=")}`);

const bytes = new Uint8Array(await request.arrayBuffer());
console.log(`body=${bytes.byteLength > 5}/${bytes.includes(255)}`);

const parsedRequest = new Request("https://example.test/upload", {
  method: "POST",
  body: form,
});
const parsed = await parsedRequest.formData();
const parsedOriginal = parsed.get("original") as File;
const parsedRenamed = parsed.get("renamed") as File;
const parsedBlob = parsed.get("blob") as File;
console.log(
  `parsed=${parsed.get("caption")};${parsedOriginal.name}/${parsedOriginal.type}/${parsedOriginal.size};` +
    `${parsedRenamed.name};${parsedBlob.name}/${await parsedBlob.text()}`,
);
console.log(
  `binary=${[...new Uint8Array(await parsedOriginal.arrayBuffer())].join(",")}`,
);

const explicit = new Request("https://example.test/upload", {
  method: "POST",
  headers: { "content-type": "application/custom" },
  body: form,
});
console.log(`explicit=${explicit.headers.get("content-type")}`);
