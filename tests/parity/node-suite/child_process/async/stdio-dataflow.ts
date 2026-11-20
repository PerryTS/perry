import { type ChildProcess, spawn } from "node:child_process";

function close(child: ChildProcess): Promise<number | null> {
  return new Promise((resolve) => child.on("close", resolve));
}

{
  const child = spawn("node", [
    "-e",
    "let text='';process.stdin.setEncoding('utf8');process.stdin.on('data',c=>text+=c);process.stdin.on('end',()=>process.stdout.write(text));",
  ]);
  let stdout = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => (stdout += chunk));
  console.log("text stdin writable before:", child.stdin.writable);
  console.log("text stdin readable:", child.stdin.readable);
  console.log("text write one:", child.stdin.write("hello"));
  console.log("text write two:", child.stdin.write(" world", "utf8"));
  child.stdin.end("!");
  console.log("text status:", await close(child));
  console.log("text stdout:", stdout);
  console.log("text stdin writable after:", child.stdin.writable);
}

{
  const child = spawn("node", [
    "-e",
    "const b=[];process.stdin.on('data',c=>b.push(c));process.stdin.on('end',()=>process.stdout.write(Buffer.concat(b).toString('hex')));",
  ]);
  let stdout = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => (stdout += chunk));
  child.stdin.write(new Uint8Array([0, 1, 127, 128]));
  child.stdin.end(Buffer.from([254, 255]));
  console.log("binary status:", await close(child));
  console.log("binary hex:", stdout);
}

{
  const producer = spawn("node", ["-e", "process.stdout.write('pipe-data')"]);
  const producerClosed = close(producer);
  const consumer = spawn("node", [
    "-e",
    "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>process.stdout.write(s.toUpperCase()));",
  ]);
  producer.stdout.pipe(consumer.stdin);
  let stdout = "";
  let stderr = "";
  consumer.stdout.setEncoding("utf8");
  consumer.stderr.setEncoding("utf8");
  consumer.stdout.on("data", (chunk: string) => (stdout += chunk));
  consumer.stderr.on("data", (chunk: string) => (stderr += chunk));
  const [producerCode, consumerCode] = await Promise.all([
    producerClosed,
    close(consumer),
  ]);
  console.log("pipe statuses:", producerCode, consumerCode);
  console.log("pipe stdout:", stdout);
  console.log("pipe stderr:", JSON.stringify(stderr));
}

{
  const child = spawn(
    "node",
    ["-e", "require('node:fs').writeSync(3, Buffer.from('fd3-data'))"],
    { stdio: ["ignore", "ignore", "ignore", "pipe"] },
  );
  let output = "";
  const fd3: any = child.stdio[3];
  fd3?.setEncoding?.("utf8");
  fd3?.on?.("data", (chunk: string) => (output += chunk));
  console.log("fd3 stdio length:", child.stdio.length);
  console.log("fd3 present:", fd3 !== null && fd3 !== undefined);
  console.log("fd3 status:", await close(child));
  console.log("fd3 output:", output);
}
