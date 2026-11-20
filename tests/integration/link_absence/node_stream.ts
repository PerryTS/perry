// HWS hook: js_nm_install_stream
// HWS prefixes: perry_runtime::node_stream|js_node_stream_|js_nm_install_events
import { Readable } from "node:stream"; console.log(Readable.from(["ok"]).read());

// This positive witness also exercises the complete module installer.
import * as moduleSurface from "node:stream";
if (Object.keys(moduleSurface).length === 0) throw new Error("empty builtin namespace");
