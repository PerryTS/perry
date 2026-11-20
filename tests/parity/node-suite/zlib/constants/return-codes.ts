import { constants } from "node:zlib";

console.log("Z_OK:", constants.Z_OK);
console.log("Z_STREAM_END:", constants.Z_STREAM_END);
console.log("Z_NEED_DICT:", constants.Z_NEED_DICT);
console.log("Z_ERRNO:", constants.Z_ERRNO);
console.log("Z_STREAM_ERROR:", constants.Z_STREAM_ERROR);
console.log("Z_DATA_ERROR:", constants.Z_DATA_ERROR);
console.log("Z_MEM_ERROR:", constants.Z_MEM_ERROR);
console.log("Z_BUF_ERROR:", constants.Z_BUF_ERROR);
console.log("Z_VERSION_ERROR:", constants.Z_VERSION_ERROR);
