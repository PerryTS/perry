import tls from "node:tls";

function result(label: string, host: any, cert: any) {
  const err = tls.checkServerIdentity(host, cert);
  if (err === undefined) {
    console.log(label + ":", "ok");
  } else {
    console.log(label + ":", err instanceof Error, err.code, err.host, err.cert === cert, typeof err.reason);
  }
}

result("dns san match", "example.com", {
  subjectaltname: "DNS:example.com",
  subject: { CN: "other.example" },
});

result("dns wildcard match", "api.example.com", {
  subjectaltname: "DNS:*.example.com",
});

result("cn fallback match", "example.com", {
  subject: { CN: "example.com" },
});

result("ip san match", "127.0.0.1", {
  subjectaltname: "IP Address:127.0.0.1",
});

result("numeric hostname coerces", 123, {
  subject: { CN: "123" },
});

result("dns san mismatch", "bad.example", {
  subjectaltname: "DNS:example.com",
  subject: { CN: "example.com" },
});

result("cn mismatch", "bad.example", {
  subject: { CN: "example.com" },
});
