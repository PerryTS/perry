import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const privateKey = `-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDzl6zuiR91Zgju
xBU31xw4TSZugr9hg0feHnyuAuxqZi32e96PpUN1TZRmqGR3OiQH/N0AEUmPjdjX
aZ0c6u6Dkx5ijBr1SpMkiBjWR+2zKfItRNwSmofcWmxzTXy+slG7f2VXmCuso4/G
cVp7KX+VApHrMg2tappsz9TGOKkJayKlVtQVou1cSU/oysIR3KEweMh9uMos/7DL
rGhXklpab1qaPEKQtL/OOrbqtFWlnBMsBUA4GVsmPR8wIdAU9UtiMgZ3bojmoRz0
BeGS+B658wS/Hcd3vCTlnXBY8PRFSkM3amBfX8VjYQ6KVapwXZBPMzv1ebcvP6Qt
h+2qZ5+DAgMBAAECggEAU9OZbkj/622em1QdHSdIjdN260bRR2RfIgAJ1fQpmX/q
R01fTL2Jll+JNz6xvBnk9l69St2oG9+rhI3SxHXQeLTzGuSuDkWIl2TCb1M3aJWB
wrRUq45EPL9dXNyIljNVTxnLLTavqOxseNTfV0zzm7rTrkV+UXRDCjkHNuOewB9T
WbPn+bavrBeQWu1ltFXVovZst6m6tlO6cmSrmM3hv8pYrRxmSIj6SNx4eyAc0wYn
JzOek0cw7ro7NHJKetK9Ni5wPY3KcPXWFObbVTA4oKSuD+Huf+JXcdVvLdkfph+/
oopAyGz5eSKmiZU3/+kwuHZolkKNZEM1hYA3bCDjYQKBgQD+KT7tmUkJ1coyum5h
s2O6yt5nQiDo4LMXWHB10R8+pylM8JSk3xeSFsKdtHv5VTy7Uv1AFq2g7ZovmMOv
CaEQ512C3M9lpVUa3krJqwLTRWnZnqLRl2tFjjKgAjgnZdA4lV9BkACcNUZd61QA
edY8B8olvrq2961oXi0CTYmGcwKBgQD1WtrHVNp1Jn+EfiCeDbnoX9A1x67IPuN9
yUj6fP0hbMhYvqFvwcN3ui9PzbRTbK6jts95kKzM8KV04+QJwxDZKCobg9nhIHGg
AOcCBVEpKk7Q7NVJpiTFGIuSoZzTX9GD8DJVo6OQOTz8MZXSip+c/2XlhYseg924
cpvZI/MusQKBgE606SbdDDA+g3I4J4yb5+tlfYAOi3ByfSNioNjrXLijPXf1HKL9
7yevYq9BwA6TZc5AwepB25z1V4Ub0qV23ukELQIkbRl2HKfIZPKUwbg5S7E3ngY3
1OFiSq0gYtFYhyWupCQCex3kpZjaElZfZIeMhf4wVVPp2Upzt456AnefAoGBAMuw
ycCCaXqoo2TTcTDGJHkOUkTTqf8EdsiOus95xIxjS1ChslSdgDF9mJmgJPy9VZ8E
veomec8KWdJY/5A7KVmfRpXhOJj13l7/YMkEsQSD4zr/43JpRE18uyLYmOHCwqXO
W3tNhxTM8BxO7hsEis5EGcwaugxzXTcrrsbuWY2BAoGBANSzDdtJrONSUVag/dMM
oeJmJOpyiAlxaiz5XeY9C3B5anSD60jOhZwvA4kiNAl1arBTsbhir/AxqSNZQ97s
WecYGb3drz0ur2usn469X9eSodCb1TI50LDgNX+WX7lEHcoI8tFv1oOFAiPISe7m
/fxuo1/hiam7dCqT/vS1R3mT
-----END PRIVATE KEY-----`;

const publicKey = `-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA85es7okfdWYI7sQVN9cc
OE0mboK/YYNH3h58rgLsamYt9nvej6VDdU2UZqhkdzokB/zdABFJj43Y12mdHOru
g5MeYowa9UqTJIgY1kftsynyLUTcEpqH3Fpsc018vrJRu39lV5grrKOPxnFaeyl/
lQKR6zINrWqabM/UxjipCWsipVbUFaLtXElP6MrCEdyhMHjIfbjKLP+wy6xoV5Ja
Wm9amjxCkLS/zjq26rRVpZwTLAVAOBlbJj0fMCHQFPVLYjIGd26I5qEc9AXhkvge
ufMEvx3Hd7wk5Z1wWPD0RUpDN2pgX1/FY2EOilWqcF2QTzM79Xm3Lz+kLYftqmef
gwIDAQAB
-----END PUBLIC KEY-----`;

const data = Buffer.from("rsa digest variant parity");

for (const alg of ["RSA-SHA256", "RSA-SHA384", "RSA-SHA512"]) {
  const sig = crypto.sign(alg, data, privateKey);
  console.log(alg + " len:", sig.length);
  console.log(alg + " ok:", crypto.verify(alg, data, publicKey, sig));
}
