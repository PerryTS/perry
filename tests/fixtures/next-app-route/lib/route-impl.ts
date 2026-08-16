import { headers } from "next/headers";
import { NextRequest, NextResponse } from "next/server";

async function handle(request: NextRequest): Promise<NextResponse> {
  const id = request.nextUrl.searchParams.get("id") ?? "missing";
  const requestedIterations = Number(request.nextUrl.searchParams.get("iterations") ?? "100");
  const iterations = Number.isInteger(requestedIterations)
    ? Math.max(1, Math.min(1_000, requestedIterations))
    : 100;

  const beforeAwait = (await headers()).get("x-request-id");
  const { checksum } = await import("./lazy-work");
  await new Promise<void>((resolve) => setTimeout(resolve, 1));
  const afterAwait = (await headers()).get("x-request-id");
  const requestBody = request.method === "POST" ? await request.text() : "";

  const payload = JSON.stringify({
    runtime: "next",
    method: request.method,
    pathname: request.nextUrl.pathname,
    id,
    iterations,
    checksum: checksum(iterations),
    beforeAwait,
    afterAwait,
    requestBody,
  });
  const bytes = new TextEncoder().encode(payload);
  const split = Math.max(1, Math.floor(bytes.length / 2));
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(bytes.subarray(0, split));
      queueMicrotask(() => {
        controller.enqueue(bytes.subarray(split));
        controller.close();
      });
    },
  });

  const response = new NextResponse(stream, {
    status: 207,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "x-perry-repro": id,
    },
  });
  response.cookies.set("perry_ctx", id, { httpOnly: true, sameSite: "strict" });
  return response;
}

export const GET = handle;
export const POST = handle;
