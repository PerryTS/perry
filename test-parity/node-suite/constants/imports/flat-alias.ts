import constantsDefault, * as constantsNs from "node:constants";
import {
  EACCES,
  F_OK,
  O_RDONLY,
  POINT_CONVERSION_COMPRESSED,
  POINT_CONVERSION_UNCOMPRESSED,
  PRIORITY_NORMAL,
  RSA_PKCS1_PADDING,
  SIGINT,
  SIGTERM,
  RTLD_DEEPBIND,
  SSL_OP_NO_SSLv2,
  SSL_OP_NO_TLSv1,
} from "node:constants";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import process from "node:process";

console.log("key count broad:", Object.keys(constantsDefault).length > 100);
console.log("F_OK:", F_OK, typeof F_OK);
console.log("O_RDONLY:", O_RDONLY, typeof O_RDONLY);
console.log("SIGINT:", SIGINT, typeof SIGINT);
console.log("SIGTERM:", SIGTERM, typeof SIGTERM);
console.log("EACCES:", EACCES, typeof EACCES);
console.log("PRIORITY_NORMAL:", PRIORITY_NORMAL, typeof PRIORITY_NORMAL);
console.log("RTLD_DEEPBIND:", RTLD_DEEPBIND, typeof RTLD_DEEPBIND);
console.log("RSA:", RSA_PKCS1_PADDING, typeof RSA_PKCS1_PADDING);
console.log("SSL_OP_NO_SSLv2:", SSL_OP_NO_SSLv2, typeof SSL_OP_NO_SSLv2);
console.log("SSL_OP_NO_TLSv1:", SSL_OP_NO_TLSv1, typeof SSL_OP_NO_TLSv1);
console.log(
  "POINT_COMPRESSED:",
  POINT_CONVERSION_COMPRESSED,
  typeof POINT_CONVERSION_COMPRESSED,
);
console.log(
  "POINT_UNCOMPRESSED:",
  POINT_CONVERSION_UNCOMPRESSED,
  typeof POINT_CONVERSION_UNCOMPRESSED,
);
console.log("default F_OK same fs:", constantsDefault.F_OK === fs.constants.F_OK);
console.log("ns F_OK same fs:", constantsNs.F_OK === fs.constants.F_OK);
console.log("SIGINT same os:", constantsDefault.SIGINT === os.constants.signals.SIGINT);
console.log("SIGTERM same os:", constantsDefault.SIGTERM === os.constants.signals.SIGTERM);
console.log("EACCES same os:", constantsDefault.EACCES === os.constants.errno.EACCES);
console.log(
  "PRIORITY same os:",
  constantsDefault.PRIORITY_NORMAL === os.constants.priority.PRIORITY_NORMAL,
);
console.log("RTLD same os:", constantsDefault.RTLD_DEEPBIND === os.constants.dlopen.RTLD_DEEPBIND);
console.log(
  "RSA same crypto:",
  constantsDefault.RSA_PKCS1_PADDING === crypto.constants.RSA_PKCS1_PADDING,
);
console.log(
  "SSL_OP_NO_SSLv2 same crypto:",
  constantsDefault.SSL_OP_NO_SSLv2 === crypto.constants.SSL_OP_NO_SSLv2,
);
console.log(
  "POINT_UNCOMPRESSED same crypto:",
  constantsDefault.POINT_CONVERSION_UNCOMPRESSED ===
    crypto.constants.POINT_CONVERSION_UNCOMPRESSED,
);
console.log("builtin module type:", typeof process.getBuiltinModule("constants"));
console.log(
  "builtin module same default:",
  process.getBuiltinModule("constants") === constantsDefault,
);
