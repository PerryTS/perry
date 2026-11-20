// A client IncomingMessage is a Readable: a 'readable' listener switches it to
// paused mode and read() drains the buffered body, pause()/resume() hold back
// and flush 'data', and readableEnded/closed/destroyed follow the stream
// lifecycle. A local server writes a chunked body in delayed pieces.
import * as http from "node:http";

const PARTS = ["alpha-", "bravo-", "charlie-", "delta"];
const FULL = PARTS.join("");

const server = http.createServer((_req, res) => {
  res.writeHead(200, { "content-type": "text/plain" });
  let i = 0;
  const next = () => {
    if (i < PARTS.length) {
      res.write(PARTS[i++]);
      setTimeout(next, 25);
    } else {
      res.end();
    }
  };
  next();
});

function get(port: number, onResponse: (res: any) => void): void {
  http.get({ port, agent: false }, onResponse);
}

function readableLoop(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      let body = "";
      let buffers = true;
      res.on("readable", () => {
        let chunk: any;
        while ((chunk = res.read()) !== null) {
          if (!Buffer.isBuffer(chunk)) buffers = false;
          body += chunk.toString();
        }
      });
      res.on("end", () => {
        console.log("a body", body, "buffers", buffers);
        console.log("a read after end", res.read());
        resolve();
      });
      console.log("a read before data", res.read());
    });
  });
}

function readSizes(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      const sizes: number[] = [];
      let body = "";
      res.on("readable", () => {
        let chunk: any;
        while ((chunk = res.read(4)) !== null) {
          sizes.push(chunk.length);
          body += chunk.toString();
        }
      });
      res.on("end", () => {
        console.log("b sizes", sizes.join(","));
        console.log("b body", body);
        resolve();
      });
    });
  });
}

function readEncoded(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      res.setEncoding("utf8");
      let body = "";
      let strings = true;
      res.on("readable", () => {
        let chunk: any;
        while ((chunk = res.read()) !== null) {
          if (typeof chunk !== "string") strings = false;
          body += chunk;
        }
      });
      res.on("end", () => {
        console.log("c body", body, "strings", strings);
        resolve();
      });
    });
  });
}

function pauseResume(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      let paused = false;
      let resumed = false;
      let dataWhilePaused = false;
      let body = "";
      res.on("data", (chunk: any) => {
        if (paused) dataWhilePaused = true;
        body += chunk.toString();
        if (!resumed && !paused) {
          console.log("d first", chunk.toString());
          paused = true;
          res.pause();
          setTimeout(() => {
            console.log("d resume with", body.length, "bytes delivered");
            paused = false;
            resumed = true;
            res.resume();
          }, 250);
        }
      });
      res.on("end", () => {
        console.log("d end after resume", resumed);
        console.log("d data while paused", dataWhilePaused);
        console.log("d body", body);
        resolve();
      });
    });
  });
}

function lifecycle(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      const state = (label: string) =>
        console.log(
          "e",
          label,
          "readable",
          res.readable,
          "ended",
          res.readableEnded,
          "closed",
          res.closed,
          "destroyed",
          res.destroyed,
        );
      state("start");
      res.on("end", () => state("end"));
      res.on("close", () => {
        state("close");
        resolve();
      });
      res.resume();
    });
  });
}

function destroyEarly(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      let destroyed = false;
      let dataAfterDestroy = false;
      res.on("data", () => {
        if (destroyed) {
          dataAfterDestroy = true;
          return;
        }
        destroyed = true;
        res.destroy();
        console.log("f destroyed", res.destroyed, "closed", res.closed);
      });
      res.on("close", () => {
        console.log("f close closed", res.closed, "destroyed", res.destroyed);
        setTimeout(() => {
          console.log("f data after destroy", dataAfterDestroy);
          resolve();
        }, 150);
      });
    });
  });
}

function removeListener(port: number): Promise<void> {
  return new Promise((resolve) => {
    get(port, (res: any) => {
      let removedCalls = 0;
      let keptBody = "";
      const removed = () => {
        removedCalls++;
      };
      res.on("data", removed);
      res.on("data", (chunk: any) => {
        keptBody += chunk.toString();
      });
      res.off("data", removed);
      res.on("end", () => {
        console.log("g removed listener calls", removedCalls);
        console.log("g body", keptBody);
        resolve();
      });
    });
  });
}

server.listen(0, async () => {
  const port = (server.address() as { port: number }).port;
  console.log("full length", FULL.length);
  await readableLoop(port);
  await readSizes(port);
  await readEncoded(port);
  await pauseResume(port);
  await lifecycle(port);
  await destroyEarly(port);
  await removeListener(port);
  server.close();
});
