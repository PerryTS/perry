// #4915: ReadableStreamBYOBReader + ByteLengthQueuingStrategy parity.
// Run with: perry <this file> vs `node --experimental-strip-types <this file>`.

async function main() {
  // ── BYOB read(view) over a byte stream with pre-enqueued chunks ──
  {
    const rs = new ReadableStream({
      type: "bytes",
      start(controller) {
        controller.enqueue(new Uint8Array([1, 2, 3, 4, 5]));
        controller.enqueue(new Uint8Array([6, 7]));
        controller.close();
      },
    });
    const reader = rs.getReader({ mode: "byob" });
    const a = await reader.read(new Uint8Array(4));
    console.log("byob-1", a.done, Array.from(a.value));
    const b = await reader.read(new Uint8Array(4));
    console.log("byob-2", b.done, Array.from(b.value));
    const c = await reader.read(new Uint8Array(4));
    console.log("byob-3", c.done, c.value.byteLength);
  }

  // ── BYOB reader on a non-byte stream throws TypeError ──
  {
    const plain = new ReadableStream({
      start(controller) {
        controller.enqueue("x");
        controller.close();
      },
    });
    try {
      plain.getReader({ mode: "byob" });
      console.log("mode-check", "no-throw");
    } catch (e: any) {
      console.log("mode-check", e instanceof TypeError);
    }
  }

  // ── byobRequest.respond from the pull source ──
  {
    let pulls = 0;
    const rs = new ReadableStream({
      type: "bytes",
      pull(controller) {
        pulls++;
        const req = controller.byobRequest;
        if (req !== null && req !== undefined) {
          const view = req.view;
          view[0] = 42;
          view[1] = 43;
          req.respond(2);
        } else {
          controller.enqueue(new Uint8Array([9]));
        }
      },
    });
    const reader = rs.getReader({ mode: "byob" });
    const r = await reader.read(new Uint8Array(8));
    console.log("respond", r.done, Array.from(r.value));
  }

  // ── ByteLengthQueuingStrategy: real desiredSize accounting ──
  {
    let ctl: any;
    const rs = new ReadableStream(
      {
        start(controller) {
          ctl = controller;
        },
      },
      new ByteLengthQueuingStrategy({ highWaterMark: 16 }),
    );
    console.log("bls-0", ctl.desiredSize);
    ctl.enqueue(new Uint8Array(6));
    console.log("bls-1", ctl.desiredSize);
    ctl.enqueue(new Uint8Array(4));
    console.log("bls-2", ctl.desiredSize);
    const reader = rs.getReader();
    await reader.read();
    console.log("bls-3", ctl.desiredSize);
  }

  // ── ByteLengthQueuingStrategy object shape ──
  {
    const s = new ByteLengthQueuingStrategy({ highWaterMark: 1024 });
    console.log("strategy", s.highWaterMark, s.size(new Uint8Array(7)));
    const c = new CountQueuingStrategy({ highWaterMark: 4 });
    console.log("count", c.highWaterMark, c.size(new Uint8Array(7)));
  }

  // ── WritableStream constructed with a ByteLengthQueuingStrategy ──
  {
    const written: number[] = [];
    const ws = new WritableStream(
      {
        write(chunk: Uint8Array) {
          written.push(chunk.byteLength);
        },
      },
      new ByteLengthQueuingStrategy({ highWaterMark: 64 }),
    );
    const writer = ws.getWriter();
    await writer.write(new Uint8Array(10));
    await writer.write(new Uint8Array(20));
    await writer.close();
    console.log("ws", written.join(","), writer.desiredSize);
  }

  // ── TransformStream accepts strategies ──
  {
    const ts = new TransformStream(
      {
        transform(chunk: Uint8Array, controller: any) {
          controller.enqueue(new Uint8Array(chunk.byteLength * 2));
        },
      },
      new ByteLengthQueuingStrategy({ highWaterMark: 32 }),
      new ByteLengthQueuingStrategy({ highWaterMark: 32 }),
    );
    const writer = ts.writable.getWriter();
    const reader = ts.readable.getReader();
    await writer.write(new Uint8Array(3));
    const out = await reader.read();
    console.log("ts", out.done, out.value.byteLength);
  }

  console.log("done");
}

main();
