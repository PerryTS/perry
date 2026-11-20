// HWS companion: url_engine
// HWS hook: js_runtime_install_url_engine
// HWS prefixes: url::|perry_runtime::url::
import { domainToASCII, fileURLToPath, pathToFileURL } from "node:url";
console.log(domainToASCII("münchen.de"));
const file = pathToFileURL("/tmp/a b.txt");
console.log(file.href, fileURLToPath(file));
