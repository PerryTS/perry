// turnloop P2: the surfaces that moved from ad-hoc threads onto the event
// loop's own handles — `node:dgram` sockets, a child's stdout/stderr pipes,
// and an OS signal delivered to `process.on`.
//
// Output is deliberately free of anything host-specific: no ports (ephemeral),
// no pids, no paths, and no errno numbers. What is asserted is the behaviour
// the migration had to preserve — which bytes arrive, in what order, with
// which metadata, and that the process still exits on its own afterwards.
import dgram from "node:dgram";
import { spawn } from "node:child_process";
import process from "node:process";

function once<T>(build: (resolve: (value: T) => void) => void): Promise<T> {
  return new Promise<T>((resolve) => build(resolve));
}

// ── dgram: a real UDP round trip on loopback ────────────────────────────────
async function udpRoundTrip(): Promise<void> {
  const receiver = dgram.createSocket("udp4");
  const messages: string[] = [];
  let sourceMatchedSender = false;

  await once<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
  const bound = receiver.address();
  console.log("dgram bound", bound.port > 0, bound.address, bound.family);

  const sender = dgram.createSocket("udp4");
  await once<void>((resolve) => sender.bind(0, "127.0.0.1", () => resolve()));
  const senderPort = sender.address().port;

  const three = once<void>((resolve) => {
    receiver.on("message", (msg, rinfo) => {
      messages.push(msg.toString());
      // rinfo has to come from the datagram itself, not from a guess: this is
      // the one field a completion-shaped receive could silently lose.
      if (rinfo.port === senderPort && rinfo.address === "127.0.0.1" && rinfo.family === "IPv4") {
        sourceMatchedSender = true;
      }
      if (messages.length === 3) resolve();
    });
  });

  // Three sends submitted back to back: the send callbacks must fire, in
  // order, and every datagram must arrive.
  const acked: string[] = [];
  for (const word of ["one", "two", "three"]) {
    sender.send(word, bound.port, "127.0.0.1", (err) => {
      acked.push(err ? "err" : word);
    });
  }
  await three;

  console.log("dgram messages", messages.join(","));
  console.log("dgram rinfo names the sender", sourceMatchedSender);
  console.log("dgram send callbacks", acked.join(","));

  // Options still reach the kernel through the copy Perry retains.
  receiver.setBroadcast(true);
  receiver.setTTL(64);
  receiver.setMulticastTTL(1);
  receiver.setMulticastLoopback(true);
  console.log("dgram options accepted", true);

  await once<void>((resolve) => {
    receiver.close(() => resolve());
  });
  await once<void>((resolve) => {
    sender.close(() => resolve());
  });
  console.log("dgram closed", true);
}

// ── child_process: real bytes on a real child's stdout and stderr ───────────
async function childStreams(): Promise<void> {
  const child = spawn(process.execPath, [
    "-e",
    "process.stdout.write('out-a');process.stdout.write('out-b');process.stderr.write('err-1');process.exit(7)",
  ]);

  let out = "";
  let err = "";
  let stdoutEnded = false;
  let stderrEnded = false;
  child.stdout.on("data", (chunk) => {
    out += chunk.toString();
  });
  child.stderr.on("data", (chunk) => {
    err += chunk.toString();
  });
  child.stdout.on("end", () => {
    stdoutEnded = true;
  });
  child.stderr.on("end", () => {
    stderrEnded = true;
  });

  const [code, signal] = await once<[number | null, string | null]>((resolve) => {
    child.on("close", (code, signal) => resolve([code, signal]));
  });

  console.log("child stdout", out);
  console.log("child stderr", err);
  console.log("child stdout ended", stdoutEnded);
  console.log("child stderr ended", stderrEnded);
  console.log("child exit", code, signal);
}

// A child that writes more than one pipe buffer: the multishot read has to
// deliver every chunk, not just the first.
async function childLargeOutput(): Promise<void> {
  const child = spawn(process.execPath, [
    "-e",
    "const line='x'.repeat(1023)+'\\n';for(let i=0;i<256;i++)process.stdout.write(line);",
  ]);
  let bytes = 0;
  let chunks = 0;
  child.stdout.on("data", (chunk) => {
    bytes += chunk.length;
    chunks += 1;
  });
  const code = await once<number | null>((resolve) => {
    child.on("close", (code) => resolve(code));
  });
  console.log("child large bytes", bytes);
  console.log("child large chunked", chunks >= 1);
  console.log("child large exit", code);
}

// ── signals: real OS signals delivered to `process.on` ─────────────────────
//
// All four names Perry can carry through turnloop plus one it cannot, so the
// test covers both transports in one program. A signal listener is
// ref-neutral — it must not by itself hold the loop open — so something else
// has to keep the process alive while each signal is in flight, which is
// exactly why Node's own documentation reaches for `process.stdin.resume()`
// in this example.
async function selfSignals(): Promise<void> {
  const seen: string[] = [];
  for (const name of ["SIGINT", "SIGTERM", "SIGHUP", "SIGUSR2", "SIGQUIT"] as const) {
    await new Promise<void>((resolve) => {
      const keepalive = setTimeout(() => resolve(), 5000);
      process.once(name, () => {
        seen.push(name);
        clearTimeout(keepalive);
        resolve();
      });
      process.kill(process.pid, name);
    });
  }
  console.log("signals delivered", seen.join(","));
  console.log("signal listeners after", process.listenerCount("SIGINT"));
}

async function main(): Promise<void> {
  await udpRoundTrip();
  await childStreams();
  await childLargeOutput();
  await selfSignals();
  console.log("done");
}

main();
