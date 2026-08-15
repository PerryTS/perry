const RETAINED_BODY = Buffer.from(JSON.stringify({
  runtime: "perry",
  iterations: 100,
  checksum: 3726872593,
}));

function response(body: Buffer): Buffer {
  const output = Buffer.alloc(5 + 2 + 4 + 4 + body.length);
  output[0] = 0x50;
  output[1] = 0x43;
  output[2] = 0x48;
  output[3] = 0x32;
  output[4] = 2;
  let offset = 5;
  output.writeUInt16BE(200, offset);
  offset += 2;
  output.writeUInt32BE(0, offset);
  offset += 4;
  output.writeUInt32BE(body.length, offset);
  offset += 4;
  body.copy(output, offset);
  return output;
}

// The issue's temporary-response shape. JSON.stringify's fresh result must
// remain rooted through Buffer.from after a host-boundary full collection.
export function handle(_frame: Buffer): Buffer {
  const body = Buffer.from(JSON.stringify({
    runtime: "perry",
    iterations: 100,
    checksum: 3726872593,
  }));
  return response(body);
}

// Classification control: the body is a module root rather than a temporary.
export function handleRetained(_frame: Buffer): Buffer {
  return response(RETAINED_BODY);
}
