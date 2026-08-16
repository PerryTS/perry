function streamedResponse(label: string): Response {
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode(`{"label":"${label}",`));
      queueMicrotask(() => {
        controller.enqueue(encoder.encode('"complete":true}'));
        controller.close();
      });
    },
  });

  const response = new Response(stream, {
    status: 207,
    statusText: "Multi-Status",
    headers: {
      "content-type": "application/json; charset=utf-8",
      "x-perry-repro": label,
    },
  });
  response.headers.append(
    "set-cookie",
    `perry_ctx=${label}; Path=/; HttpOnly; SameSite=Strict`,
  );
  return response;
}

export function syncResponse(): Response {
  return streamedResponse("sync");
}

export async function asyncResponse(): Promise<Response> {
  await Promise.resolve();
  return streamedResponse("async");
}

export class NextLikeResponse extends Response {
  readonly nextMarker = "next-like";

  constructor(body?: BodyInit | null, init?: ResponseInit) {
    super(body, init);
  }
}

export async function asyncNextResponse(): Promise<NextLikeResponse> {
  await Promise.resolve();
  const encoder = new TextEncoder();
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encoder.encode('{"label":"next",'));
      queueMicrotask(() => {
        controller.enqueue(encoder.encode('"complete":true}'));
        controller.close();
      });
    },
  });
  const response = new NextLikeResponse(stream, {
    status: 207,
    statusText: "Multi-Status",
    headers: {
      "content-type": "application/json; charset=utf-8",
      "x-perry-repro": "next",
    },
  });
  response.headers.append(
    "set-cookie",
    "perry_ctx=next; Path=/; HttpOnly; SameSite=Strict",
  );
  return response;
}

export function erroredResponse(): Response {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      queueMicrotask(() => controller.error(new Error("stream-boom")));
    },
  });
  return new Response(stream);
}
