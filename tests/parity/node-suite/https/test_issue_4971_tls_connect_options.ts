// #4971 — tls.connect(options) overload + https idle-connection close path.
//
// Mirrors Node's test-https-server-close-idle: an https server gets two raw
// TLS clients. client1 sends only a partial request line (it is "currently
// sending a request" — NOT idle); client2 completes a keep-alive request and
// then sits idle. `server.close()` must destroy only the idle client2 socket;
// `server.closeAllConnections()` then takes client1 down too.
//
// Pre-fix, `tls.connect({ port, rejectUnauthorized: false })` string-coerced
// the options object (NA_STR table row), returned a NaN-boxed NULL handle,
// and every `client.on(...)`/`client.write(...)` tripped the runtime's
// [NULL_PTR_METHOD_CALL] guard while the test hung forever. The https server
// additionally had no connection tracking at all: no 'connection' events and
// closeAllConnections/closeIdleConnections as silent no-ops.

import { createServer } from "node:https";
import { connect } from "node:tls";

const key = `-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCZdknk+UU6sHsu
dx2QKZbcNO4sH1GZ/xyXhzSerQuHJ3MufBDqq7SgI7W3AanlomfGy/f4qnPkM1MQ
JNzhtsJ9w/CRHlzBFrKik1rSegss6DvSxBYVijv8lYipPJaOmvgZ0n3aRKBSO7ZV
DmESLRxq4eBr8hpL6bm58XpHP4EO3ZONGPjcCpOBlQ47xm7FEhqh7wDYtHbMaMl9
ItjfbUkPxpeINOxROpcxZEsi/w1zzh6JZrDyxTZUS0aM2oyx0pE1b/9c7IUrCLIg
VKYP+zuzcLh476DBZyjV3rZ/wrytEfF7qy1/emCN4JNrsfdcLimgMiHI2xrcvRSE
Qbdw71gVAgMBAAECggEAHnkeec0k2d9fCo5FLNIRXqdVDyZl8BA4R3+l64ddtWgY
R2gD/PF9M9p7dDxslgiellt84WBJlIw7h4ZmZRzDOLGOpOZ0UTRWYxGjNI3fB7sS
3Aqrfvn86O5xnXeGRwmPUCNb8dp0QngQgAnTrUYPcUrqo0zHO4FNK9b/asP5tu9q
rFSYdoYYZRAUEdjhHsAnbqsL0NXbPQhaAPLlXJfyY74s/3fhxz3IM/NVI62D2nAY
RY4jNb2Hq8Vse8vhskJ30v7Jx/fm/FIWidBODGETM598ATTQWy8ZJ2jlEjAwit9G
RxTq+HQXMLVFn6GBntIiz8a6w6dcugxouI3LDIKY6QKBgQDGNWVaoMz+TcYDjNLt
XoSsUZalo3CAhRPP0P2zvsvX+juFWxvnJ2fobZe3MMyRBmKgXDzxVivB/MkgbLM6
9UpC+/i03BHgTDiVArnC0L2tWISAAV2W8TdeYL5fS/mFrxpk3IsSKndZaZ/JSOQk
jprUgxmcB2ouhRoTG0eJUcCACQKBgQDGNPEUsh1CXNHHRAXYSXzCuQwQywk2LcRd
TDqUsxLv0akYRYCqGgBAt8d2KIbgTPF57Kb1EQaBUvKZ2UUd6LsnnW3uiffcrc9O
GD4iZNNJ/ih6DtVHen+e+TgxVUB+S4zRkW9kSA1RdGVBCEyMF6D2VAhWQtZ3BfP8
3yECesLCrQKBgQCOqbIo+CJ0TABhX8QWC/kMmrEGycvZBXAMHY3uCT9pVffvdXNw
/mEA35jaxyoGnITyjVFkF7TpLIyLZRHgNttbuUb6zoejXNlBD7Qq79oGYfcEt3bo
hPhoWtPLfcC8oxspS8Bhs+Uxmx/iXi+vzGDO4wnUz1Vy5GSvKexkf05CGQKBgC9A
n9jXPbJ8fmaLCPmvS1cA1qeKP//ymUXEzpJ0vqb9zNpEd5AV8sl7BspcjwsaTNdM
W+FA1dQu+jdDXP7sZPHkzjh4G+c4aJutm+KHNvgE55Fxx9bqlVJJB+R69o0lZcTw
byXxJ3urzBfc6qLbXzxafEJUXNyzRp+acjwtGBFhAoGBAIZMg+6vO9lrT8cWYm8L
o6x16raQ5j6pD1TJOR5pZ7r5RJ5qV7l1xr6kPAbZVrYtB5v2Zadr2L+5Pg2DnGfI
Qf1AWmqoqZzUqMRTl9ccGZ+VRCxs6RfqCLJ+uoTnL9ofvb66HBl9mbmCrv89/w5z
ARi9DXMIk3COw3E7osC7sMFc
-----END PRIVATE KEY-----`;

const cert = `-----BEGIN CERTIFICATE-----
MIIDJzCCAg+gAwIBAgIUDJiFqrHvxns50S82U2/TF+Pmyd0wDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MCAXDTI2MDYxMTA3MTQ1OVoYDzIxMjYw
NTE4MDcxNDU5WjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwggEiMA0GCSqGSIb3DQEB
AQUAA4IBDwAwggEKAoIBAQCZdknk+UU6sHsudx2QKZbcNO4sH1GZ/xyXhzSerQuH
J3MufBDqq7SgI7W3AanlomfGy/f4qnPkM1MQJNzhtsJ9w/CRHlzBFrKik1rSegss
6DvSxBYVijv8lYipPJaOmvgZ0n3aRKBSO7ZVDmESLRxq4eBr8hpL6bm58XpHP4EO
3ZONGPjcCpOBlQ47xm7FEhqh7wDYtHbMaMl9ItjfbUkPxpeINOxROpcxZEsi/w1z
zh6JZrDyxTZUS0aM2oyx0pE1b/9c7IUrCLIgVKYP+zuzcLh476DBZyjV3rZ/wryt
EfF7qy1/emCN4JNrsfdcLimgMiHI2xrcvRSEQbdw71gVAgMBAAGjbzBtMB0GA1Ud
DgQWBBS0hSQZgeoLIYQud2S7NhU1Me0aBzAfBgNVHSMEGDAWgBS0hSQZgeoLIYQu
d2S7NhU1Me0aBzAPBgNVHRMBAf8EBTADAQH/MBoGA1UdEQQTMBGCCWxvY2FsaG9z
dIcEfwAAATANBgkqhkiG9w0BAQsFAAOCAQEAM+QXKOl6u8jFMrIzODBXKDQlAjro
ROH9WmaFw4QzLsobWniQOOOrzfRZJohVlbMxBmcF4gtQB7i2tcCYKwDZe9phiAKX
eqsZiRjWLvY0U5mvfY29CAiKRCKBpXNjnJamz7Epk19peza2e1jx7ZP+dswcIgy3
QjEFEQQ6k1QkL4jPYnRc9joH/pu8FUr4iBBYzy7oeU8AgjIm34RDwPpMNHoz73VI
pnKoYubWNYsbPJCGZx1JracfY5XlaLkWUZoq/vGM0EfScUKnBlP5tSai+8R9dIEd
pSmi20rjXYZDwqE2tjZyGHvl2L5r62A8rpUL/ocBeY23MkxrJKvqT248+A==
-----END CERTIFICATE-----`;

let connections = 0;
let client1Closed = false;
let client2Closed = false;

const server = createServer({ key, cert }, (req: any, res: any) => {
  res.writeHead(200, { Connection: "keep-alive" });
  res.end();
});

server.on("connection", () => {
  connections++;
});

server.listen(0, () => {
  const port = server.address().port;

  // client1: starts a request but never finishes the head — NOT idle.
  const client1 = connect({ port, rejectUnauthorized: false });
  console.log("client1 typeof:", typeof client1);

  client1.on("connect", () => {
    console.log("client1 connected");
    client1.write("GET / HTTP/1.1");

    // client2: completes a keep-alive request, then sits idle.
    const client2 = connect({ port, rejectUnauthorized: false });
    let response = "";

    client2.on("data", (chunk: any) => {
      response += chunk.toString("utf8");
      if (response.endsWith("0\r\n\r\n") || response.indexOf("200") >= 0) {
        console.log("client2 got response:", response.indexOf("200 OK") >= 0);
        console.log("connections:", connections);

        // Node 19+: close() destroys idle keep-alive sockets only.
        server.close();

        setTimeout(() => {
          console.log("after close: client1Closed =", client1Closed);
          console.log("after close: client2Closed =", client2Closed);
          server.closeAllConnections();
          setTimeout(() => {
            console.log("after closeAll: client1Closed =", client1Closed);
            process.exit(0);
          }, 500);
        }, 500);
      }
    });

    client2.on("close", () => {
      client2Closed = true;
    });

    client2.on("connect", () => {
      client2.write("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
    });
  });

  client1.on("close", () => {
    client1Closed = true;
  });
  client1.on("error", () => {});
});
