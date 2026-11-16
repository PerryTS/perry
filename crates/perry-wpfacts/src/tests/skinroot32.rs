//! SKINDES2 fixtures (three.js `AnimationMixer._addInactiveBinding`,
//! three.core.js:55566-55580): `bindingsByRoot[ rootUuid ]` and
//! `bindingByName[ trackName ] = binding` with `rootUuid` a generateUUID()
//! string (table parts + '-' literals, then `toLowerCase`, 22.1.3.28: '-'
//! maps to itself) and `trackName` = name + '.' + suffix (13.15.3: the
//! literal '.' is in the result). No Object.prototype member name and not
//! `__proto__` contains '-' or '.', so the read is own-or-absent and the
//! store is an own data store. Differential: each arm is compared with the
//! same program whose keys are exact constants ("k-1.position").
//! RED on c06442a6c3: r1/r2 (the store key is `Key::Any(0)`: the store path
//! never reads the key node's string entries, store.rs:599).
use super::*;

/// Receivers (class names) and verdicts of every non-call `.q` / `.r` read.
fn reads(pf: &ProgramFacts) -> Vec<String> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| (s.key == "q" || s.key == "r") && !s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.iter()
        .map(|s| {
            let mut r: Vec<String> = s
                .receivers
                .iter()
                .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
                .collect();
            r.sort();
            r.dedup();
            format!("{}:{r:?}:{:?}:top={}", s.key, s.verdict, s.top)
        })
        .collect()
}

/// `uuid` / `name`: the key expressions passed to `add`; `lower`: the uuid
/// travels through `toLowerCase` inside `generateUUID`.
fn prog(uuid: &str, name: &str) -> String {
    format!(
        "class B {{ q = 2; }}
         class M {{ r = 3; }}
         const lut: string[] = [];
         for (let i = 0; i < 256; i++) lut[i] = (i < 16 ? '0' : '') + i.toString(16);
         function generateUUID(): string {{
           const d0 = Math.random() * 0xffffffff | 0;
           const d1 = Math.random() * 0xffffffff | 0;
           const uuid = lut[d0 & 0xff] + lut[d0 >> 8 & 0xff] + '-' + lut[d1 & 0xff] + lut[d1 >> 8 & 0xff];
           return uuid.toLowerCase();
         }}
         class Root {{ uuid: string; name: string; constructor(n: string) {{ this.uuid = generateUUID(); this.name = n; }} }}
         const root = new Root(String(Date.now()));
         class Mixer {{
           byRoot: any = {{}};
           add(binding: any, rootUuid: string, trackName: string) {{
             const bindingsByRoot = this.byRoot;
             let bindingByName: any = bindingsByRoot[rootUuid];
             if (bindingByName === undefined) {{ bindingByName = new M(); bindingsByRoot[rootUuid] = bindingByName; }}
             bindingByName[trackName] = binding;
           }}
         }}
         const mixer = new Mixer();
         const track = {{ name: {name} }};
         mixer.add(new B(), {uuid}, track.name);
         const got: any = mixer.byRoot[{uuid}];
         console.log(got.r);
         const b: any = got[track.name];
         console.log(b.q);
         const h: any = Object.prototype.hasOwnProperty;
         console.log(h.q, h.r);"
    )
}

const EXACT_UUID: &str = "'ab-cd'";
const EXACT_NAME: &str = "'k-1.position'";

fn same_as_exact(uuid: &str, name: &str) {
    let want = reads(&facts(&prog(EXACT_UUID, EXACT_NAME)));
    let got = reads(&facts(&prog(uuid, name)));
    eprintln!("SKINROOT32 exact={want:?}\nSKINROOT32 arm({uuid},{name})={got:?}");
    assert_eq!(got, want, "uuid {uuid} name {name}");
}

/// r1: the uuid literal-table + '-' shape, the name built with '.'.
#[test]
fn r1_dash_uuid_dot_name_is_own_data() {
    same_as_exact("generateUUID()", "root.name + '.' + 'position'");
}

/// r2: the uuid through an object field and a parameter (root.uuid, made
/// in the constructor; `add`'s parameter), `toLowerCase` inside.
#[test]
fn r2_field_param_lowercase_uuid_is_own_data() {
    same_as_exact(
        "root.uuid",
        "(root.name ? root.name : root.uuid) + '.' + 'position'",
    );
}

/// k1: keys from JSON content alone: any string, may be `__proto__` or
/// `toString`: the arm differs from the exact one (wide, as today).
#[test]
fn k1_json_keys_stay_wide() {
    let want = reads(&facts(&prog(EXACT_UUID, EXACT_NAME)));
    let j = "JSON.parse(process.argv[2]).k";
    let got = reads(&facts(&prog(j, j)));
    eprintln!("SKINROOT32 k1={got:?}");
    assert_ne!(got, want);
}

/// k2: the receiver's chain has a member whose name contains '.': a key
/// "contains '.'" may name it, so that member is seen.
#[test]
fn k2_chain_member_with_dot_is_seen() {
    let src = "class P { 'x.q'() { return 1; } }
               class B { q = 2; }
               const o: any = new P();
               const k = String(Date.now()) + '.q';
               const f: any = o[k];
               console.log(f.q);";
    let pf = facts(src);
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "q" && !s.is_call)
        .flat_map(|s| s.receivers.clone())
        .collect();
    eprintln!("SKINROOT32 k2={v:?}");
    assert!(
        v.iter().any(|r| r.contains("x.q") || r.contains("P")),
        "{v:?}"
    );
}

/// k3: the store key is the literal "__proto__": the prototype store is
/// seen (differs from the exact own-key arm).
#[test]
fn k3_literal_proto_store_is_seen() {
    let want = reads(&facts(&prog(EXACT_UUID, EXACT_NAME)));
    let got = reads(&facts(&prog("'__proto__'", "'__proto__'")));
    eprintln!("SKINROOT32 k3={got:?}");
    assert_ne!(got, want);
}
