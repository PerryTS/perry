import * as net from "node:net";

// #2013: socket.setTimeout(msecs) validates `msecs` like Node: a non-number
// throws TypeError [ERR_INVALID_ARG_TYPE]; NaN / Infinity / a negative value
// throws RangeError [ERR_OUT_OF_RANGE].
const badMsecs: any[] = [true, null, {}, -1, NaN, Infinity, -5.5];

for (const value of badMsecs) {
  try {
    const sock = new net.Socket();
    sock.setTimeout(value);
    console.log("setTimeout", String(value), "=> NO THROW");
  } catch (err: any) {
    console.log("setTimeout", String(value), "=>", err.name, err.code, "|", err.message);
  }
}

// Valid msecs are accepted and setTimeout returns the socket (chainable).
const sock = new net.Socket();
console.log("setTimeout(1000) === socket:", sock.setTimeout(1000) === sock);
console.log("setTimeout(0) ok:", typeof sock.setTimeout(0));
