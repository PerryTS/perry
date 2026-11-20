import * as tls from "node:tls";

const key = `-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC142pbDBURXgUm
cd8BxJE5lzeV+7H0gOYvpfl/fVIMZYu6d3k8eslXgnyxaKPKfTHCpANgy3H6FO5i
wpUtL5Kcl2Z+2W/w9niK0yHY+x8nLykTaCdF++cWhshGryAIaYPBgJP53xzcW1U+
am1UbpLSYbpu6dfLcMi9TC7ofLTRo02ItIhv9IzNgQ+6V0MMHMrFY5UMP10t6Qol
Vuddng4xxteHCN6599sqm1XWYhkIHMp7gk9NJRA0ri5dsoIGgBrUORlO7oGDGWtD
dxqTK73yKiE/PZSKyJuYcpRxo9Z+UU0J4vdeJ+cX696OXQtpJogh7wrOwjr0GWI1
Jo07sY2bAgMBAAECggEAGOOPcaRTxlvHDk2Fq7XB/jnxgBiqxLTWCrjGTR+RGHjV
Z8TNIQKs8MhpF1OkxLgLP5b25AoNb3WUGzfaIY6Wxs5hgZkaXBmLQXUw9q0s7vfh
Qd0ThrGOG8MsvkiyHu86pBdb6FS98Rn0WQVHhEavLjzzkxV06HXnRMlFyp1fQXzr
yM06ciZ9wQ/p4qsS5UswdfvG6TE2AtF16FWC+gIbGQJl7BCqRG3fsmcDZiYe4mWp
Pme/wb3g0HF/p5gK9nFe9U/ZYc9mks+FGpPCayHjyl/9+B7kutwQCdo/jB1vKwQJ
mjH9/0nmTHnru41iOwMYpksjiUq3T5s5RghjtWI90QKBgQDk5bjHV7kAOl0/IFcB
cg2wa5tp0rOMDOu+CW5jkEHix5XdHGUMbsiImf+KJq+sVK5MrPLvShQzSB/G2fnd
B/FRQu0u39rZbvEG8HDwhNJ3dUVZRN9mco5sih0Tlgdok7u4lROQdkEFZ5+ahaI1
midMEqq/J+L87nIQz+ALKyqDKwKBgQDLbMS/Z4R5S3vEnU3Vj9sgdthOLRKX4MoL
kQln4heJPTAcBr0zBU3HK1kmaqlZw7r32cT+tirWs4VgSO3OUg+Z9NMWV+TLzHJF
yUz/alwHC5gcGyI2g2zaGW9l2h9V3NQbioMmG1welPLqAkGHRUxsphzErTT4GkIl
3oMsBIenUQKBgQC2umTeTjtT4UPbRxfuAXzIH787pYbMAOyZErJbLShLwAT1NNu+
JxpTYozLXsLTEe7rKw3s1Ph3T9Z+SjjbqKGOu5zY1L/C4HvtjDi86WuTDb1E3GRz
RnRIVaGMpzJW28j6O5gYtS6HAAg7tP6fR+ajJivE2jSssjXBEhHLGLShbQKBgEEz
tcRb27w9E4irmt0O5P982EwGamU/6cLXVBp1/3E/qYHyLwaBdrKWFFcZ7PoWoID8
zgWOQiDbHa8E8SQmbVW9gUMyHOWtvBreMM3VO3YOo0yu7cJnUaZ+bJRK26xbwaiq
Nusp7dbniwyyeGpxLdPNUn8/vTCgyf71WTnsocZhAoGAU/7q0pUHfJvTL/RnnLrP
NqU8hFOhvIzjs3P93eJS9Jv7w2ec4gkd1CgD0VclDCsXdPrv6kNSqJjWvL3A8IO5
gnIB8E4KbRmL3gY9batwbvip4iqctJcd5F7F2Bz+nB7i1ZN1pIYKkAHG8fAHgOpB
jPuc5bizot150Jj2Zt4OTB0=
-----END PRIVATE KEY-----`;

const cert = `-----BEGIN CERTIFICATE-----
MIIDJTCCAg2gAwIBAgIUDhz63FcsENjICzINODuFnM9HL+EwDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDUzMTA5Mjg1OFoXDTI2MDYw
MTA5Mjg1OFowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEAteNqWwwVEV4FJnHfAcSROZc3lfux9IDmL6X5f31SDGWL
und5PHrJV4J8sWijyn0xwqQDYMtx+hTuYsKVLS+SnJdmftlv8PZ4itMh2PsfJy8p
E2gnRfvnFobIRq8gCGmDwYCT+d8c3FtVPmptVG6S0mG6bunXy3DIvUwu6Hy00aNN
iLSIb/SMzYEPuldDDBzKxWOVDD9dLekKJVbnXZ4OMcbXhwjeuffbKptV1mIZCBzK
e4JPTSUQNK4uXbKCBoAa1DkZTu6BgxlrQ3cakyu98iohPz2UisibmHKUcaPWflFN
CeL3XifnF+vejl0LaSaIIe8KzsI69BliNSaNO7GNmwIDAQABo28wbTAdBgNVHQ4E
FgQU4Hcv4VMFi8JVbg9nJsMMsqZy4zgwHwYDVR0jBBgwFoAU4Hcv4VMFi8JVbg9n
JsMMsqZy4zgwDwYDVR0TAQH/BAUwAwEB/zAaBgNVHREEEzARgglsb2NhbGhvc3SH
BH8AAAEwDQYJKoZIhvcNAQELBQADggEBAHPox5+qbXT37jahDP0N17J16r8RBZcq
4rMgyPkX/X0w+Iv8OeTuSEZOLBkOEO474gMeLzIB1rrbcMxEqpyN26VkLUIOPMyF
A94B01SqauPVdBotHjC+nhTIziRyQoGvqj51ciQz11dLfrI4xpSKy3YpVRbJsSOH
HNl+nU1pIlWxQt1cU4HAW6hxohdpHkOXpCov2i4clInLBerp9JMLoMbsevnmhYpD
7nL4yOIqi5uAhYqxxuV1zu0WzFnpShqRQK3Io0xX7fh5NAuOJQNWdmeebVI5Gmgs
rjiP4BrZOGQViJ3XvKxsZAmYNU4fIqB4im+LJ78bqFmPkP4B4Cc2Dt8=
-----END CERTIFICATE-----`;

let sawServer = false;
let sawClient = false;
let sawData = false;
let closed = false;
let ticks = 0;

const server = tls.createServer({ key, cert }, (socket) => {
  sawServer = true;
  const cipher = socket.getCipher();
  console.log(
    "tls server secureConnection:",
    socket.encrypted,
    typeof socket.getProtocol(),
    typeof cipher.name,
    typeof socket.getPeerCertificate(),
  );
  console.log("tls server session:", socket.getSession().length >= 0);
  console.log("tls server fragment:", socket.setMaxSendFragment(1200));
  socket.end("ok");
});

server.on("error", (err) => {
  console.log("tls server error:", err.code || err.message);
});

server.listen(0, () => {
  const addr = server.address();
  console.log("tls server address:", typeof addr.port, addr.port > 0, typeof addr.address);
  const keys = server.getTicketKeys();
  console.log("tls server ticket keys:", keys.length);
  server.setTicketKeys(keys);

  const client =
    tls.connect.length >= 4
      ? tls.connect("127.0.0.1", addr.port, "localhost", 0)
      : tls.connect({
          host: "127.0.0.1",
          port: addr.port,
          servername: "localhost",
          rejectUnauthorized: false,
        });

  client.on("secureConnect", () => {
    sawClient = true;
    const cipher = client.getCipher();
    console.log(
      "tls client secureConnect:",
      client.encrypted,
      typeof client.getProtocol(),
      typeof cipher.name,
      client.getSession().length >= 0,
      client.exportKeyingMaterial(8, "perry").length,
      client.setMaxSendFragment(1200),
    );
  });

  client.on("data", (buf) => {
    sawData = true;
    console.log("tls client data:", buf.toString());
  });

  client.on("error", (err) => {
    console.log("tls client error:", err.code || err.message);
  });

  client.on("close", () => {
    console.log("tls client close:", sawClient, sawServer, sawData);
    server.close(() => {
      closed = true;
      console.log("tls server closed:", !server.listening);
    });
  });
});

setInterval(() => {
  ticks += 1;
  if (closed) {
    process.exit(0);
  }
  if (ticks > 150) {
    console.log("tls timeout:", sawClient, sawServer, sawData);
    process.exit(1);
  }
}, 20);
