//! WHATWG URL, Encoding and HTML timers: conversions, dictionary reads,
//! callback argument flow, and conservative replacement/unknown paths.
use super::*;
fn check(body: &str, yes: bool) {
    let src = format!(r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.marker; }}
        const o: any = new Svc();
        {body}
        console.log(g(o));
    "#);
    super::dump_helper_fixture(&src);
    let pf = facts(&src); let v = &site(&pf, "marker").verdict;
    assert_eq!(!matches!(v, Verdict::No(_)), yes, "{body}: {v:?}");
}
#[test]
fn url_methods_ignore_extra_arguments() { check("const u: any = new URL('https://example.org/'); u.toString(o); u.toJSON(o);", true); }
#[test]
fn url_setters_only_stringify() { check("const u = new URL('https://example.org/'); u.pathname = o; u.search = o; u.hash = o;", true); }
#[test]
fn url_parse_only_stringifies() { check("URL.canParse(o); URL.parse(o);", true); }
#[test]
fn url_search_params_only_stringify() { check("const p = new URLSearchParams(); p.append(o, o); p.set(o, o); p.get(o); p.getAll(o); p.has(o); p.delete(o); p.toString(); p.sort();", true); }
#[test]
fn url_search_params_callback_result_ignored() { check("const p = new URLSearchParams('a=b'); p.forEach(() => o);", true); }
#[test]
fn encoder_only_stringifies_source() { check("const e = new TextEncoder(); e.encode(o); try { e.encodeInto(o, new Uint8Array(40)); } catch (_err) {}", true); }
#[test]
fn decoder_only_reads_buffer_slots() { check("const d = new TextDecoder(); try { d.decode(o); } catch (_e) {}", true); }
#[test]
fn timer_methods_ignore_arguments() { check("const t: any = setTimeout(() => {}, 0); t.unref(o); t.ref(o); t.refresh(o); t.hasRef(o); clearTimeout(t);", true); }
#[test]
fn clear_timer_only_coerces_id() { check("clearTimeout(o); clearInterval(o);", true); }
#[test]
fn timer_delay_runs_coercion() { check("o.valueOf = function() { this.leak(); return 0; }; const t = setTimeout(() => {}, o); clearTimeout(t);", false); }
#[test]
fn timer_extra_arguments_reach_callback() { check("setTimeout((x: any) => x.leak(), 0, o);", false); }
#[test]
fn timer_immediate_arguments_reach_callback() { check("setImmediate((x: any) => x.leak(), o);", false); }
#[test]
fn url_replaced_method_stays_conservative() { check("const u: any = new URL('https://example.org/'); u.toString = (x: any) => x.leak(); u.toString(o);", false); }
#[test]
fn encoder_runs_source_to_string() { check("o.toString = function() { this.leak(); return ''; }; new TextEncoder().encode(o);", false); }
#[test]
fn decoder_constructor_reads_label() { check("o.toString = function() { this.leak(); return 'utf-8'; }; new TextDecoder(o);", false); }
#[test]
fn decoder_options_getter_runs() { check("const d: any = new TextDecoder(); Object.defineProperty(o, 'stream', {get() { o.leak(); return false; }}); d.decode(new Uint8Array(), o);", false); }
#[test]
fn params_callback_this_arg_is_passed() { check("const p = new URLSearchParams('a=b'); p.forEach(function() { this.leak(); }, o);", false); }
#[test]
fn web_unknown_receiver_stays_conservative() { check("const x: any = (globalThis as any).unknown; x.encode(o);", false); }

#[test]
fn timer_registration_ignores_callback_result() {
    check("const t = setTimeout(() => o, 0); const i = setInterval(() => o, 0); clearTimeout(t); clearInterval(i);", true);
}
#[test]
fn timer_replaced_handle_method_stays_conservative() {
    check("const t: any = setTimeout(() => {}, 0); t.unref = (x: any) => x.leak(); t.unref(o); clearTimeout(t);", false);
}
#[test]
fn codec_replaced_method_stays_conservative() {
    check("const e: any = new TextEncoder(); e.encode = (x: any) => x.leak(); e.encode(o);", false);
}
#[test]
fn params_replaced_method_stays_conservative() {
    check("const p: any = new URLSearchParams(); p.append = (x: any) => x.leak(); p.append(o, 'x');", false);
}
