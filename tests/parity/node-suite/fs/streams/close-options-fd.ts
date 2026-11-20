import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_stream_close_options";
try { fs.rmSync(ROOT, { recursive: true, force: true }); } catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const p = ROOT + "/read.txt";
fs.writeFileSync(p, "abc");

await new Promise<void>((resolve) => {
  const events: string[] = [];
  const rs = fs.createReadStream(p);
  rs.on("end", () => events.push("end"));
  rs.on("close", () => {
    events.push("close");
    console.log("read close order:", events.join(">"));
    resolve();
  });
  rs.resume();
});

await new Promise<void>((resolve) => {
  let close = false;
  const rs = fs.createReadStream(p, { emitClose: false });
  rs.on("close", () => { close = true; });
  rs.on("end", () => {
    setTimeout(() => {
      console.log("read emitClose false:", close);
      resolve();
    }, 5);
  });
  rs.resume();
});

const fd = fs.openSync(p, "r");
await new Promise<void>((resolve) => {
  const rs = fs.createReadStream("", { fd, autoClose: false });
  rs.on("end", () => {
    setTimeout(() => {
      let alive = false;
      try {
        fs.fstatSync(fd);
        alive = true;
      } catch (_e) {}
      console.log("read autoClose false fd alive:", alive);
      fs.closeSync(fd);
      resolve();
    }, 5);
  });
  rs.resume();
});

const out = ROOT + "/write.txt";
await new Promise<void>((resolve) => {
  let close = false;
  const ws = fs.createWriteStream(out, { emitClose: false });
  ws.on("close", () => { close = true; });
  ws.on("finish", () => {
    setTimeout(() => {
      console.log("write emitClose false:", close);
      resolve();
    }, 5);
  });
  ws.end("x");
});
