// HWS hook: js_runtime_install_event_target
// HWS prefixes: js_abort_|js_event_new|js_event_subclass_init|js_event_target_|js_custom_event_|js_dom_exception_|perry_runtime::event_target::
const t=new EventTarget(); t.addEventListener("hws",()=>console.log("event")); t.dispatchEvent(new Event("hws"));
