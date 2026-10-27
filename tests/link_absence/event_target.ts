// HWS hook: none (no hook yet)
// HWS prefixes: js_abort_|perry_runtime::event_target::
const t=new EventTarget(); t.addEventListener("hws",()=>console.log("event")); t.dispatchEvent(new Event("hws"));
