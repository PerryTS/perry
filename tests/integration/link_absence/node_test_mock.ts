// HWS companion: mod_node_test
// HWS hook: js_runtime_install_mod_node_test
// HWS prefixes: perry_runtime::node_submodules::node_test|perry_runtime::node_test::
import {mock} from "node:test";
const fn=mock.fn((value:number)=>value*2);console.log(fn(21),fn(3));
console.log(fn.mock.callCount(),fn.mock.calls.length);
