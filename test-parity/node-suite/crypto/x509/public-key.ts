import { KeyObject, X509Certificate } from "node:crypto";

const pem = `-----BEGIN CERTIFICATE-----
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

const cert = new X509Certificate(pem);
const key = cert["publicKey"];
const exported = key.export({ type: "spki", format: "pem" }) as string;

console.log("publicKey type:", key.type);
console.log("publicKey asymmetric:", key.asymmetricKeyType);
console.log("publicKey details type:", typeof key.asymmetricKeyDetails);
console.log("publicKey instanceof:", key instanceof KeyObject);
console.log("export starts:", exported.startsWith("-----BEGIN PUBLIC KEY-----\n"));
console.log("export ends:", exported.endsWith("-----END PUBLIC KEY-----\n"));
console.log("export line count:", exported.trimEnd().split("\n").length);
console.log("equals repeated:", key.equals(cert["publicKey"]));
