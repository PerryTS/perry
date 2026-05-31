import { Buffer } from "node:buffer";
import { X509Certificate } from "node:crypto";

function show(label: string, input: string | Buffer) {
  try {
    const cert = new X509Certificate(input);
    console.log(label, "ok", typeof cert, cert === undefined);
  } catch (err: any) {
    console.log(label, "throw", err.name, typeof err.message, String(err.message).length > 0);
  }
}

show("string", "not a cert");
show("buffer", Buffer.from("not a cert"));
show("short-pem", "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----");
