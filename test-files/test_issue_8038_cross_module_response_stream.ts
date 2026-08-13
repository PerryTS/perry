// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_SCHEDULE_SEED=8038 PERRY_GC_SCHEDULE_RATE=1

import {
  asyncNextResponse,
  asyncResponse,
  erroredResponse,
  NextLikeResponse,
  syncResponse,
} from "./_helpers/issue_8038_response_producer.ts";

declare function gc(): void;

function forceGc(): void {
  const churn: Array<{ index: number; payload: string }> = [];
  for (let index = 0; index < 2_000; index += 1) {
    churn.push({ index, payload: `response-root-${index}` });
  }
  if (typeof gc === "function") gc();
}

async function validate(label: string, response: Response): Promise<void> {
  const firstHeaders = response.headers;
  const firstBody = response.body;
  // Keep the response, Headers view, ReadableStream, queued chunk/controller,
  // and later the reader live across both scheduled and explicit collections.
  forceGc();
  console.log(`${label}:brand=${response instanceof Response}`);
  console.log(`${label}:status=${response.status} ${response.statusText}`);
  console.log(`${label}:headers-stable=${firstHeaders === response.headers}`);
  console.log(`${label}:body-stable=${firstBody === response.body}`);
  console.log(`${label}:content-type=${firstHeaders.get("content-type")}`);
  console.log(`${label}:x-perry-repro=${firstHeaders.get("x-perry-repro")}`);
  console.log(`${label}:cookie=${firstHeaders.get("set-cookie")}`);

  if (firstBody === null) {
    throw new Error(`${label}: missing body`);
  }
  const reader = firstBody.getReader();
  forceGc();
  const decoder = new TextDecoder();
  let body = "";
  let chunks = 0;
  let eof = false;
  while (!eof) {
    const result = await reader.read();
    forceGc();
    eof = result.done;
    if (!result.done) {
      chunks += 1;
      body += decoder.decode(result.value);
    }
  }
  console.log(`${label}:chunks=${chunks} eof=${eof} body=${body}`);
}

async function main(): Promise<void> {
  const initializerHeaders = new Headers({ x: "a" });
  const copiedHeadersResponse = new Response(null, {
    headers: initializerHeaders,
  });
  console.log(
    `headers-copy:identity=${copiedHeadersResponse.headers === initializerHeaders}`,
  );
  initializerHeaders.set("x", "b");
  console.log(`headers-copy:response=${copiedHeadersResponse.headers.get("x")}`);
  copiedHeadersResponse.headers.set("x", "c");
  console.log(`headers-copy:initializer=${initializerHeaders.get("x")}`);

  await validate("sync", syncResponse());
  await validate("async", await asyncResponse());
  const next = await asyncNextResponse();
  console.log(`next:subclass-brand=${next instanceof NextLikeResponse}`);
  console.log(`next:marker=${next.nextMarker}`);
  await validate("next", next);

  const response = erroredResponse();
  if (response.body === null) throw new Error("error: missing body");
  const reader = response.body.getReader();
  try {
    await reader.read();
    console.log("error:missing");
  } catch (error) {
    console.log(`error:surfaced=${String(error)}`);
  }
}

export function runIssue8038(): Promise<void> {
  return main();
}

// App-only dylib hosts call the exported wrapper and root its returned Promise.
// Ordinary executable and Node parity runs retain the normal top-level entry.
if (process.env.PERRY_ISSUE_8038_LIBRARY_HOST !== "1") {
  runIssue8038();
}
