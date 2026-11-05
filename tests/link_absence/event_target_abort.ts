// HWS companion: event_target
// HWS hook: js_runtime_install_event_target
// HWS prefixes: js_abort_|js_event_new|js_event_subclass_init|js_event_target_|js_custom_event_|js_dom_exception_|perry_runtime::event_target::
const Parent=EventTarget;class Child extends Parent{}
const target=new Child();target.addEventListener("hws",event=>console.log((event as CustomEvent).detail),{once:true});
target.dispatchEvent(new CustomEvent("hws",{detail:7}));target.dispatchEvent(new Event("hws"));
const controller=new AbortController();controller.signal.addEventListener("abort",()=>console.log(controller.signal.reason));controller.abort("stop");
const error=new DOMException("message","AbortError");console.log(error.name,error.message,error.code);
