// Regression: Body.formData() must select the multipart parser from the
// Content-Type header. It previously fed every body to the URL-encoded parser,
// turning an upload's first boundary line into a bogus key and dropping files.

const boundary = "----perryMultipart9617";
const body =
  `--${boundary}\r\n` +
  `Content-Disposition: form-data; name="caption"\r\n\r\n` +
  `ein rotes Rad\r\n` +
  `--${boundary}\r\n` +
  `content-disposition: form-data; name="photo"; filename="bike.jpg"\r\n` +
  `content-type: image/jpeg\r\n\r\n` +
  `BINARYDATA\r\n` +
  `--${boundary}\r\n` +
  `Content-Disposition: form-data; name="caption"\r\n\r\n` +
  `zweites Rad\r\n` +
  `--${boundary}--\r\n`;

async function inspect(label: string, owner: Request | Response) {
  const form = await owner.formData();
  const photo = form.get("photo") as File;
  console.log(`${label}:keys=${[...form.keys()].join(",")}`);
  console.log(
    `${label}:caption=${form.get("caption")};all=${form.getAll("caption").join("|")}`,
  );
  console.log(
    `${label}:file=${photo instanceof File}/${photo instanceof Blob}/${photo.name}/${photo.type}/${photo.size}/${await photo.text()}`,
  );
  console.log(
    `${label}:entries=${[...form.entries()].map(([key, value]) => `${key}:${typeof value}`).join(",")}`,
  );
  console.log(
    `${label}:values=${[...form.values()].map((value) => typeof value).join(",")}`,
  );
  const seen: string[] = [];
  form.forEach((value, key) => seen.push(`${key}:${typeof value}`));
  console.log(`${label}:forEach=${seen.join(",")};used=${owner.bodyUsed}`);
}

await inspect(
  "request",
  new Request("https://example.test/upload", {
    method: "POST",
    headers: { "content-type": `multipart/form-data; boundary="${boundary}"` },
    body,
  }),
);

await inspect(
  "response",
  new Response(body, {
    headers: { "content-type": `multipart/form-data; boundary=${boundary}` },
  }),
);

const mutableHeaders = new Request("https://example.test/mutated", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body,
});
mutableHeaders.headers.set(
  "content-type",
  `multipart/form-data; boundary=${boundary}`,
);
console.log(
  `mutated-header=${(await mutableHeaders.formData()).get("caption")}`,
);

const encoded = await new Request("https://example.test/form", {
  method: "POST",
  headers: { "content-type": "application/x-www-form-urlencoded; charset=UTF-8" },
  body: "name=Perry+TS&name=second&empty=",
}).formData();
console.log(
  `urlencoded=${encoded.getAll("name").join("|")}/${JSON.stringify(encoded.get("empty"))}`,
);

for (const contentType of ["application/json", "multipart/form-data"]) {
  const request = new Request("https://example.test/bad", {
    method: "POST",
    headers: { "content-type": contentType },
    body: "not a form",
  });
  try {
    await request.formData();
    console.log(`bad:${contentType}:resolved`);
  } catch (error) {
    console.log(
      `bad:${contentType}:${error instanceof TypeError}:used=${request.bodyUsed}`,
    );
  }
}
