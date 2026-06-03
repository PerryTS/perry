import { X509Certificate } from "node:crypto";

const sanPem = `-----BEGIN CERTIFICATE-----
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

const cnOnlyPem = `-----BEGIN CERTIFICATE-----
MIIDXzCCAkegAwIBAgIUICGaY01t7nWGtQ+QsMAicwoeRLUwDQYJKoZIhvcNAQEL
BQAwPzELMAkGA1UEBhMCVVMxCzAJBgNVBAgMAkNBMQ4wDAYDVQQKDAVQZXJyeTET
MBEGA1UEAwwKcGVycnkudGVzdDAeFw0yNjA1MjMyMjM3NDZaFw0yNzA1MjMyMjM3
NDZaMD8xCzAJBgNVBAYTAlVTMQswCQYDVQQIDAJDQTEOMAwGA1UECgwFUGVycnkx
EzARBgNVBAMMCnBlcnJ5LnRlc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEK
AoIBAQDSofrefQLAkx4k9mHYD/VrTLCkiPH7DP3RTmTD8UotAG+kv2+JQVIRWJOP
/mJWC+ZnIVK7dCs8fqvsHS3HuU5BAYPQ4U7IyFyA48/ZBdsHECY6wuqhNW9yD5Pj
x066iEFMckKKCNBP7gLX3rsrp4R5uWmvmK6lNqMgO4Xx8c3ae9xyxupUaS13fNzA
inw5NNp7axLJm62llWMBOP+w2ZgQL4UmJDdxe5GI0q94ChHTU7uIr3DMOGAGWuoY
zXLk8LeSwncgWn3CZZ4WpUxibNvhVG1pmZAbgeWB5GZboUMXd2a0Uyjq3EB2kYfx
hPQYOp3obhEy1JtodmJHAlqYqG8vAgMBAAGjUzBRMB0GA1UdDgQWBBRB2mlHlpxU
LohkBQ2NH8rRhV9hQDAfBgNVHSMEGDAWgBRB2mlHlpxULohkBQ2NH8rRhV9hQDAP
BgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQCvghtILxg/BdFTS9ZA
VarJrhSrEHSlJ2Bqp6v8BJmMpouU9heVhT24RdAx9nM46jZPem6Mt55ZQ+eyR7vK
iC5T5yPWreaKEWnbS7YhxSTcLZOhMMZ4eah02f1lhYtiDF6u7p6zfY1HjZlJrbE4
WNdOEcttJh8BTciSV2wuxD7iZNejN5L1wH+PATHTnuEuryksRhky4tOb7UiY7qkj
M11jjUSojXBNqw754Na4vTz5hhjJo17NeB3hZw6N+r9flCGdTQZ4k02hx9+LbxcJ
uVylGMusIXcohDauuPEBi/NXk0owHqF6uafjjW5lHK2CnsHKL8U0qHRdcKFJZ4Jx
/gTV
-----END CERTIFICATE-----`;

const sanCert = new X509Certificate(sanPem);
const cnOnlyCert = new X509Certificate(cnOnlyPem);

function report(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(`${label}:`, "err", err.name, err.code ?? "", err.message);
  }
}

console.log("typeof checkHost:", typeof sanCert["checkHost"]);
console.log("typeof checkEmail:", typeof sanCert["checkEmail"]);
console.log("typeof checkIP:", typeof sanCert["checkIP"]);

report("san host exact", () => sanCert["checkHost"]("perry-extension.test"));
report("san host alt", () => sanCert["checkHost"]("www.perry-extension.test"));
report("san host uppercase", () => sanCert["checkHost"]("PERRY-EXTENSION.TEST"));
report("san host miss", () => sanCert["checkHost"]("missing.test"));

report("san email exact", () => sanCert["checkEmail"]("admin@perry-extension.test"));
report("san email uppercase", () => sanCert["checkEmail"]("ADMIN@PERRY-EXTENSION.TEST"));
report("san email miss", () => sanCert["checkEmail"]("root@perry-extension.test"));

report("san ip exact", () => sanCert["checkIP"]("127.0.0.1"));
report("san ip miss", () => sanCert["checkIP"]("127.0.0.2"));

report("cn host exact", () => cnOnlyCert["checkHost"]("perry.test"));
report("cn host uppercase", () => cnOnlyCert["checkHost"]("PERRY.TEST"));
report("cn host miss", () => cnOnlyCert["checkHost"]("www.perry.test"));

report("checkHost missing", () => sanCert["checkHost"]());
report("checkEmail missing", () => sanCert["checkEmail"]());
report("checkIP missing", () => sanCert["checkIP"]());
