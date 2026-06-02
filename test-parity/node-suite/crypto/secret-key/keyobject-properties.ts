import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = crypto.createSecretKey(Buffer.from("Hello World"));
console.log("secret type:", key.type);
console.log("secret size:", key.symmetricKeySize);
console.log("secret asym type:", key.asymmetricKeyType);
console.log("secret asym details:", key.asymmetricKeyDetails);
console.log("secret export:", key.export().toString());
console.log("secret export buffer:", key.export({ format: "buffer" }).toString());
const jwk = key.export({ format: "jwk" });
console.log("secret jwk:", jwk.kty, jwk.k);
console.log("secret equals same:", crypto.createSecretKey(Buffer.from("Hello World")).equals(key));
console.log("secret equals different:", key.equals(crypto.createSecretKey(Buffer.from("Other"))));
console.log("hmac with secret:", crypto.createHmac("sha256", key).update("abc").digest("hex"));
