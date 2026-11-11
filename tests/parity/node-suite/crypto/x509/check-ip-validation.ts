import { X509Certificate } from "node:crypto";

const pem = `-----BEGIN CERTIFICATE-----
MIIEEDCCAvigAwIBAgIUSXW0OKKEkcMY0kWj2AXQh+WVEWEwDQYJKoZIhvcNAQEL
BQAwSTELMAkGA1UEBhMCVVMxCzAJBgNVBAgMAkNBMQ4wDAYDVQQKDAVQZXJyeTEd
MBsGA1UEAwwUcGVycnktZXh0ZW5zaW9uLnRlc3QwHhcNMjYwNjAzMTAwMTM4WhcN
MjcwNjAzMTAwMTM4WjBJMQswCQYDVQQGEwJVUzELMAkGA1UECAwCQ0ExDjAMBgNV
BAoMBVBlcnJ5MR0wGwYDVQQDDBRwZXJyeS1leHRlbnNpb24udGVzdDCCASIwDQYJ
KoZIhvcNAQEBBQADggEPADCCAQoCggEBAKFD+QNwqsoE5dEnFVP/3zMQq5CAya+y
vskPkxapgwYPhyW86Hr23oUwqazBi3B4Q5KuYXmnQYfJnM+d+pniDO9YGbBX8ao8
Xb2fKC2BQP3t8JzCr56u6c0ob0MWuLpB51A4Fj56D/kfHthUet8Z2plYKyyUPUsB
XZibgxx9COwi+NN4FsT0rDz8Vtgr3ssHPDcEZi9XTSqeooIv3Wsm2oCRhkuplqbe
Dd7Wm5rUpwZiR5ahsmP0G1qgwA55Exs++kijL3+Qg1C5MpLqYAE927TsfIqNxijy
ScaNCiUB7YxhVB3bRF+5G1KMfwBxkfEfKZ1nB0k6rpXXPlxSZ1giLxECAwEAAaOB
7zCB7DCBlAYDVR0RBIGMMIGJghRwZXJyeS1leHRlbnNpb24udGVzdIIYd3d3LnBl
cnJ5LWV4dGVuc2lvbi50ZXN0hwR/AAABhxAAAAAAAAAAAAAAAAAAAAABgRphZG1p
bkBwZXJyeS1leHRlbnNpb24udGVzdIYjaHR0cHM6Ly9wZXJyeS1leHRlbnNpb24u
dGVzdC9zdGF0dXMwJwYDVR0lBCAwHgYIKwYBBQUHAwEGCCsGAQUFBwMCBggrBgEF
BQcDAzALBgNVHQ8EBAMCBaAwHQYDVR0OBBYEFJxWlUjxxiBuCawipcrkJX8MJi+C
MA0GCSqGSIb3DQEBCwUAA4IBAQAKrCMj6C6qVPZcpCVUZT7Ez1/v2ewTBo/r5UnH
j3V2u7//9F9JE7w+iuljUcZuuyVG67DqFynhbpTu5FDlHbbMDmmNcF6XNZ0PUk+N
ROm2v3W7WAKvToyuGJAs+cQrd4JL2r3/CGNk5lkh0Q7LF1ZPtxUvIEWMKvg/tVu2
VJ6ezkG2NJt4xbgu7v/FuJnPD1LXn3gogk8bMn52DbRQFs24Jtb6Ods+ptecRokp
hQEUyxl4qRwtjtKdbE63O80yaZS+hK00zwdTkaKgddWgGEn2nI2E1fBXjBZz5JB/
0va/LS/0Zxi1rpJIIkybLVdENUaQRJXUZeYjmO2oWQQXHSqo
-----END CERTIFICATE-----`;

const cert = new X509Certificate(pem);

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(`${label}:`, "err", err.name, err.code ?? "", err.message);
  }
}

report("ipv4 exact", () => cert["checkIP"]("127.0.0.1"));
report("ipv4 miss", () => cert["checkIP"]("127.0.0.2"));
report("ipv6 compressed", () => cert["checkIP"]("::1"));
report("ipv6 expanded", () => cert["checkIP"]("0:0:0:0:0:0:0:1"));
report("ipv6 miss", () => cert["checkIP"]("0:0:0:0:0:0:0:2"));

report("invalid dotted", () => cert["checkIP"]("999.0.0.1"));
report("invalid padded", () => cert["checkIP"]("127.000.000.001"));
report("invalid text", () => cert["checkIP"]("localhost"));
report("invalid empty", () => cert["checkIP"](""));
report("invalid spaced", () => cert["checkIP"](" 127.0.0.1 "));

report("missing", () => cert["checkIP"]());
report("number", () => cert["checkIP"](1 as any));
report("object", () => cert["checkIP"]({} as any));
