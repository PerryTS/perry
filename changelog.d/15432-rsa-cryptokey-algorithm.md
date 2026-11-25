- **WebCrypto RSA keys.** `CryptoKey.algorithm` of an RSASSA-PKCS1-v1_5, RSA-PSS
  or RSA-OAEP key has `modulusLength` and `publicExponent`, for generated and
  imported keys. Libraries that check the modulus length, such as jose, accept
  Perry's RSA keys (#15177).
