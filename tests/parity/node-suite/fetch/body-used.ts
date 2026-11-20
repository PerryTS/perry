function requestWithBody(body: string): Request {
  return new Request("https://example.test/body-used", {
    method: "POST",
    body,
    duplex: "half",
  } as any);
}

function errorName(e: any): string {
  return e?.name ?? e?.constructor?.name ?? typeof e;
}

function errorMessage(e: any): string {
  return e?.message ?? String(e);
}

async function reportRejected(label: string, promise: Promise<unknown>) {
  try {
    await promise;
    console.log(`${label}: ok`);
  } catch (e: any) {
    console.log(`${label}:`, errorName(e), errorMessage(e));
  }
}

function reportThrown(label: string, action: () => unknown) {
  try {
    action();
    console.log(`${label}: ok`);
  } catch (e: any) {
    console.log(`${label}:`, errorName(e), errorMessage(e));
  }
}

async function requestTextAndClone() {
  const req = requestWithBody("request text");
  const clone = req.clone();
  console.log("Request text before:", req.bodyUsed, clone.bodyUsed);
  console.log("Request text first:", await req.text(), req.bodyUsed, clone.bodyUsed);
  console.log("Request clone text:", await clone.text(), req.bodyUsed, clone.bodyUsed);
  await reportRejected("Request text second", req.text());
}

async function responseTextAndClone() {
  const res = new Response("response text");
  const clone = res.clone();
  console.log("Response text before:", res.bodyUsed, clone.bodyUsed);
  console.log("Response text first:", await res.text(), res.bodyUsed, clone.bodyUsed);
  console.log("Response clone text:", await clone.text(), res.bodyUsed, clone.bodyUsed);
  await reportRejected("Response text second", res.text());
}

async function otherConsumers() {
  const reqJson = requestWithBody(JSON.stringify({ request: true }));
  console.log("Request json before:", reqJson.bodyUsed);
  console.log("Request json first:", (await reqJson.json()).request, reqJson.bodyUsed);
  await reportRejected("Request json second", reqJson.arrayBuffer());

  const reqArrayBuffer = requestWithBody("abc");
  console.log("Request arrayBuffer before:", reqArrayBuffer.bodyUsed);
  console.log(
    "Request arrayBuffer first:",
    (await reqArrayBuffer.arrayBuffer()).byteLength,
    reqArrayBuffer.bodyUsed,
  );
  await reportRejected("Request arrayBuffer second", reqArrayBuffer.text());

  const resJson = new Response(JSON.stringify({ response: true }));
  console.log("Response json before:", resJson.bodyUsed);
  console.log("Response json first:", (await resJson.json()).response, resJson.bodyUsed);
  await reportRejected("Response json second", resJson.text());

  const resArrayBuffer = new Response("abcd");
  console.log("Response arrayBuffer before:", resArrayBuffer.bodyUsed);
  console.log(
    "Response arrayBuffer first:",
    (await resArrayBuffer.arrayBuffer()).byteLength,
    resArrayBuffer.bodyUsed,
  );
  await reportRejected("Response arrayBuffer second", resArrayBuffer.text());

  const resBlob = new Response("blob-body", {
    headers: { "content-type": "text/plain" },
  });
  console.log("Response blob before:", resBlob.bodyUsed);
  const blob = await resBlob.blob();
  console.log("Response blob first:", blob.size, await blob.text(), resBlob.bodyUsed);
  await reportRejected("Response blob second", resBlob.text());
}

async function cloneAfterUsed() {
  const req = requestWithBody("request clone");
  await req.text();
  reportThrown("Request clone after used", () => req.clone());

  const res = new Response("response clone");
  await res.text();
  reportThrown("Response clone after used", () => res.clone());
}

await requestTextAndClone();
await responseTextAndClone();
await otherConsumers();
await cloneAfterUsed();
