import * as url from "node:url";
import { webcrypto } from "node:crypto";
class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(url.URL === URL, url.URLSearchParams === URLSearchParams);
console.log(webcrypto.subtle.constructor === SubtleCrypto);
