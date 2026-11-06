import { spawn } from "node:child_process";

const serverSource = String.raw`
const dgram = require("node:dgram");
const mode = process.argv[1];
const u16 = (value) => { const out = Buffer.alloc(2); out.writeUInt16BE(value); return out; };
const u32 = (value) => { const out = Buffer.alloc(4); out.writeUInt32BE(value); return out; };
const name = (value) => Buffer.concat(value.split(".").map((part) => Buffer.concat([Buffer.from([Buffer.byteLength(part)]), Buffer.from(part)])).concat(Buffer.from([0])));
const text = (...parts) => Buffer.concat(parts.map((part) => Buffer.concat([Buffer.from([Buffer.byteLength(part)]), Buffer.from(part)])));
const record = (type, data, ttl = 60) => Buffer.concat([Buffer.from([0xc0, 0x0c]), u16(type), u16(1), u32(ttl), u16(data.length), data]);
const records = (type) => {
  const map = {
    1: [record(1, Buffer.from([203, 0, 113, 7]), 120)],
    2: [record(2, name("ns.example.test"))],
    5: [record(5, name("alias.example.test"))],
    6: [record(6, Buffer.concat([name("ns.example.test"), name("hostmaster.example.test"), u32(1), u32(2), u32(3), u32(4), u32(5)]))],
    12: [record(12, name("ptr.example.test"))],
    15: [record(15, Buffer.concat([u16(10), name("mail.example.test")]))],
    16: [record(16, text("alpha", "beta"))],
    28: [record(28, Buffer.from("20010db8000000000000000000000007", "hex"), 240)],
    33: [record(33, Buffer.concat([u16(10), u16(20), u16(443), name("service.example.test")]))],
    35: [record(35, Buffer.concat([u16(1), u16(2), text("S", "SIP+D2U", "!^.*$!"), name("replacement.example.test")]))],
    52: [record(52, Buffer.concat([Buffer.from([3, 1, 1]), Buffer.from("abcd", "hex")]))],
    257: [record(257, Buffer.concat([Buffer.from([128, 5]), Buffer.from("issueca.example")]))],
  };
  return type === 255 ? [map[1][0], map[28][0], map[15][0], map[2][0], map[16][0], map[12][0], map[6][0], map[257][0]] : (map[type] || []);
};
function start() {
  const socket = dgram.createSocket("udp4");
  socket.on("message", (request, remote) => {
    const labels = [];
    let cursor = 12;
    while (request[cursor] !== 0) {
      const length = request[cursor++];
      labels.push(request.subarray(cursor, cursor + length).toString("ascii"));
      cursor += length;
    }
    const end = cursor + 1;
    const type = request.readUInt16BE(end);
    process.stdout.write("QUERY:" + labels.join(".") + "\n");
    if (mode === "silent") return;
    const question = request.subarray(12, end + 4);
    const answers = ["nxdomain", "nodata", "refused"].includes(mode)
      ? []
      : mode === "idna" && labels.join(".") !== "xn--maana-pta.example"
      ? [record(1, Buffer.from([203, 0, 113, 8]), 120)]
      : records(type);
    const rcode = mode === "nxdomain" ? 3 : mode === "refused" ? 5 : 0;
    const header = Buffer.concat([request.subarray(0, 2), Buffer.from([0x81, 0x80 | rcode]), u16(1), u16(answers.length), u16(0), u16(0)]);
    socket.send(Buffer.concat([header, question, ...answers]), remote.port, remote.address);
  });
  socket.bind(0, "127.0.0.1", () => {
    process.stdout.write("READY:" + socket.address().port + "\n");
  });
  return socket;
}
const socket = start();
process.on("SIGTERM", () => socket.close(() => process.exit(0)));
`;

export async function startDnsServer(mode = "answer") {
  const child = spawn("node", ["-e", serverSource, mode], {
    stdio: ["ignore", "pipe", "inherit"],
  });
  const lines = [];
  const waiters = [];
  let buffered = "";
  let readyResolve;
  let readyReject;
  const ready = new Promise((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });

  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffered += chunk;
    while (buffered.includes("\n")) {
      const index = buffered.indexOf("\n");
      const line = buffered.slice(0, index);
      buffered = buffered.slice(index + 1);
      if (line.startsWith("READY:")) readyResolve(Number(line.slice(6)));
      else if (waiters.length) waiters.shift()(line);
      else lines.push(line);
    }
  });
  child.once("error", readyReject);
  child.once(
    "exit",
    (code) => readyReject(new Error(`DNS server exited before ready: ${code}`)),
  );

  const port = await ready;
  return {
    port,
    nextQuery() {
      if (lines.length) return Promise.resolve(lines.shift());
      return new Promise((resolve) => waiters.push(resolve));
    },
    async close() {
      if (child.exitCode !== null) return;
      const exited = new Promise((resolve) => child.once("exit", resolve));
      child.kill();
      await exited;
    },
  };
}
