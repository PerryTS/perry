import { Buffer } from "node:buffer";
import { X509Certificate } from "node:crypto";

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
const toLegacy = cert["toLegacyObject"];

console.log("signatureAlgorithm:", cert["signatureAlgorithm"]);
console.log("signatureAlgorithmOid:", cert["signatureAlgorithmOid"]);
console.log("typeof toLegacyObject:", typeof toLegacy);

if (typeof toLegacy !== "function") {
  console.log("legacy unavailable");
} else {
  const legacy = toLegacy();
  console.log("legacy subject CN:", legacy["subject"]["CN"]);
  console.log("legacy issuer CN:", legacy["issuer"]["CN"]);
  console.log("legacy ca:", legacy["ca"]);
  console.log("legacy bits:", legacy["bits"]);
  console.log("legacy exponent:", legacy["exponent"]);
  console.log("legacy modulus prefix:", legacy["modulus"].slice(0, 16));
  console.log("legacy pubkey buffer:", Buffer.isBuffer(legacy["pubkey"]), legacy["pubkey"].length);
  console.log("legacy raw equals:", Buffer.isBuffer(legacy["raw"]), legacy["raw"].length === cert["raw"].length);
  console.log(
    "legacy fingerprints:",
    legacy["fingerprint"] === cert["fingerprint"],
    legacy["fingerprint256"] === cert["fingerprint256"],
    legacy["fingerprint512"] === cert["fingerprint512"],
  );
  console.log("legacy serial:", legacy["serialNumber"] === cert["serialNumber"]);
  console.log("legacy validity:", legacy["valid_from"], legacy["valid_to"]);
  console.log(
    "legacy undefined own:",
    ["infoAccess", "asn1Curve", "nistCurve", "subjectaltname", "ext_key_usage"]
      .map((key) => `${Object.prototype.hasOwnProperty.call(legacy, key)}:${String(legacy[key])}`)
      .join(","),
  );
}
