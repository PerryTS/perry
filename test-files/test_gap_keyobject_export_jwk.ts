// KeyObject.export({ format: "jwk" }) returns a plain JWK object, not a string.
import { createPrivateKey, createPublicKey, generateKeyPairSync } from "node:crypto";

const shape = (jwk: any) => `${typeof jwk} ${Object.keys(jwk).join(",")}`;

const rsa = generateKeyPairSync("rsa", { modulusLength: 2048, publicExponent: 0x10001 });
const rsaPrivate: any = rsa.privateKey.export({ format: "jwk" });
const rsaPublic: any = rsa.publicKey.export({ format: "jwk" });
console.log("rsa private", shape(rsaPrivate), rsaPrivate.kty);
console.log("rsa public", shape(rsaPublic), rsaPublic.kty);
console.log("rsa public matches private", rsaPublic.n === rsaPrivate.n && rsaPublic.e === rsaPrivate.e);

const ec = generateKeyPairSync("ec", { namedCurve: "P-256" });
const ecPrivate: any = ec.privateKey.export({ format: "jwk" });
const ecPublic: any = ec.publicKey.export({ format: "jwk" });
console.log("ec private", shape(ecPrivate), ecPrivate.kty, ecPrivate.crv);
console.log("ec public", shape(ecPublic), ecPublic.kty, ecPublic.crv);
console.log("ec public matches private", ecPublic.x === ecPrivate.x && ecPublic.y === ecPrivate.y);

// Keys built from PEM and from a JWK export the same JWK back.
const fromPem = createPrivateKey(ec.privateKey.export({ type: "pkcs8", format: "pem" }));
const fromPemJwk: any = fromPem.export({ format: "jwk" });
console.log("createPrivateKey(pem)", shape(fromPemJwk), fromPemJwk.d === ecPrivate.d);
const derived: any = createPublicKey(fromPem).export({ format: "jwk" });
console.log("createPublicKey(private)", shape(derived), derived.x === ecPrivate.x);
const roundTrip: any = createPrivateKey({ key: rsaPrivate, format: "jwk" }).export({ format: "jwk" });
console.log("jwk round trip", shape(roundTrip), roundTrip.d === rsaPrivate.d);

// pem exports stay strings.
const pem = ec.publicKey.export({ type: "spki", format: "pem" });
console.log("pem", typeof pem, String(pem).startsWith("-----BEGIN PUBLIC KEY-----"));
