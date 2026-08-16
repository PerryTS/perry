import type { FixtureNextRequest } from "./request.ts";

function churn(seed: number): number {
  const allocations: Array<{ index: number; label: string; pad: number[] }> = [];
  for (let index = 0; index < 300; index += 1) {
    allocations.push({
      index,
      label: "request-churn-" + seed + "-" + index,
      pad: [seed, index, seed + index],
    });
  }
  return allocations.length;
}

function snapshot(request: FixtureNextRequest): string {
  return [
    request.method,
    request.url,
    request.nextUrl.pathname,
    request.nextUrl.searchParams.get("id"),
    request.nextUrl.searchParams.get("iterations"),
    request.headers.get("x-request-id"),
  ].join("|");
}

async function handle(request: FixtureNextRequest): Promise<Record<string, unknown>> {
  const beforeAwait = snapshot(request);
  const { checksum } = await import("./lazy_work.ts");
  const iterations = Number(request.nextUrl.searchParams.get("iterations"));
  const checksumValue = checksum(iterations);
  const allocationCount = churn(iterations);
  await new Promise<void>((resolve) => setTimeout(resolve, 1));
  const afterAwait = snapshot(request);
  const requestBody = request.method === "POST" ? await request.text() : "";
  const method = request.method;
  const pathname = request.nextUrl.pathname;
  const header = request.headers.get("x-request-id");
  const id = request.nextUrl.searchParams.get("id");

  return {
    beforeAwait,
    afterAwait,
    method,
    pathname,
    id,
    header,
    iterations,
    checksum: checksumValue,
    allocationCount,
    requestBody,
  };
}

export function syncSummary(request: FixtureNextRequest): string {
  churn(17);
  return snapshot(request);
}

export const GET = handle;
export const POST = handle;
