// Fetch's bad-port policy applies to initial URLs and followed redirects.
// Port 0 is covered in Rust: Fetch blocks it, but Node 26.5.1 does not.

async function rejection(url: string, redirect: RequestRedirect = "follow"): Promise<string> {
  try {
    const response = await fetch(url, { redirect });
    await response.text();
    return "unexpected response " + response.status;
  } catch (error: any) {
    return error.name + "|" + error.message + "|" + error.cause.name + "|" +
      error.cause.message + "|" + String(error.cause.code);
  }
}

async function main() {
  const ports = [1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43,
    53, 69, 77, 79, 87, 95, 101, 102, 103, 104, 109, 110, 111, 113, 115, 117,
    119, 123, 135, 137, 139, 143, 161, 179, 389, 427, 465, 512, 513, 514, 515,
    526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993,
    995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566,
    6665, 6666, 6667, 6668, 6669, 6679, 6697, 10080];
  const expected = "TypeError|fetch failed|Error|bad port|undefined";
  let passed = 0;
  for (const scheme of ["http", "https"]) {
    for (const port of ports) {
      const result = await rejection(scheme + "://127.0.0.1:" + port + "/");
      if (result === expected) passed++;
      else console.log("mismatch", scheme, port, result);
    }
  }
  console.log("blocked", passed);
  console.log("normalized", await rejection("http://[::1]:00025/"));

  // The Rust integration harness supplies a real HTTP server. The standalone
  // gap fixture needs no listener or port reservation.
  const origin = process.argv[2];
  if (!origin) return;
  const allowed = await fetch(origin + "/allowed");
  console.log("allowed", allowed.status, await allowed.text());
  console.log("follow", await rejection(origin + "/redirect"));
  const manual = await fetch(origin + "/redirect", { redirect: "manual" });
  console.log("manual", manual.status, manual.headers.get("location"));
  await manual.text();
  console.log("error", await rejection(origin + "/redirect", "error"));
}
main();
