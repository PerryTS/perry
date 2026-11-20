//! Image replay R1: the shape records of the mint record are minted again through the runtime's
//! own mint, in mint order, with the id, semantic-generation and prototype-serial counters
//! positioned before each draw. It is enter step 3b: after the canonical-keys replay (3a), whose
//! trie the canonical key lists name, and before the root words (step 4), which rewrite the
//! descriptor id cells and the counters with their cut values. The record stores no counter: the
//! image's changed data words hold each at the cut, and the replay asserts them before step 4.
#![cfg_attr(not(test), allow(dead_code))]

use super::format::{
    MintRec, MINT_ROUTE_DYNAMIC, MINT_ROUTE_LAZY, MINT_ROUTE_RUNTIME, MINT_ROUTE_STATIC,
};
use crate::object::layout_desc as ld;
use crate::object::shapes as sh;

/// One process counter a mint draws from: name, address, width in bytes and value.
pub(crate) struct Counter {
    pub(crate) name: String,
    pub(crate) addr: usize,
    pub(crate) width: usize,
    pub(crate) value: u64,
}

/// What the replay takes besides the records: each record's key list and prototype word resolved
/// in this process, the start of the data window (`desc_off` is relative to it) and the image's
/// changed data words as (8-byte aligned address, value).
pub(crate) struct Inputs<'a> {
    pub(crate) keys_at: &'a dyn Fn(usize, &MintRec) -> usize,
    pub(crate) proto_word: &'a dyn Fn(usize, &MintRec) -> u64,
    pub(crate) data_lo: usize,
    pub(crate) words: &'a [(usize, u64)],
}

fn read_counter(addr: usize, width: usize) -> u64 {
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
    // SAFETY: the address of a process static counter of that width (`snap_image_counter_cells`).
    unsafe {
        if width == 4 {
            u64::from(AtomicU32::from_ptr(addr as *mut u32).load(Relaxed))
        } else {
            AtomicU64::from_ptr(addr as *mut u64).load(Relaxed)
        }
    }
}

/// The counters and their values now.
pub(crate) fn counters_now() -> Vec<Counter> {
    sh::snap_image_counter_cells()
        .into_iter()
        .map(|(name, addr, width)| Counter {
            value: read_counter(addr, width),
            name,
            addr,
            width,
        })
        .collect()
}

/// The counter's value in the word list: the bytes of the word that covers it, or None.
fn counter_in_words(c: &Counter, words: &[(usize, u64)]) -> Option<u64> {
    let at = c.addr & !7;
    let w = words.iter().find(|w| w.0 == at)?.1;
    let mask = if c.width >= 8 {
        u64::MAX
    } else {
        (1u64 << (c.width * 8)) - 1
    };
    Some(w >> ((c.addr - at) * 8) & mask)
}

/// F3: after the replay each counter equals its value in the word list, or its value at the
/// begin (`begin`) when no changed word covers it.
pub(crate) fn snap_image_counters_check(
    begin: &[Counter],
    words: &[(usize, u64)],
) -> Result<(), String> {
    for c in begin {
        let want = counter_in_words(c, words).unwrap_or(c.value);
        let now = read_counter(c.addr, c.width);
        if now != want {
            return Err(format!(
                "snap image: mint replay: counter {} differs: {now} after the replay, {want} in the image",
                c.name
            ));
        }
    }
    Ok(())
}

/// Replay `recs` (blob order is mint order): pass 1 mints each record, pass 2 sets the handles
/// (their targets are minted by then), then the counters are asserted against the word list.
pub(crate) fn snap_image_mint_replay(recs: &[MintRec], inp: &Inputs) -> Result<(), String> {
    let _gc = crate::gc::GcSuppressScope::new();
    let begin = counters_now();
    let mut protos: Vec<u64> = Vec::new();
    for (i, r) in recs.iter().enumerate() {
        mint_one(i, r, inp, &mut protos)?;
    }
    for (i, r) in recs.iter().enumerate() {
        fill_handles(i, r)?;
    }
    snap_image_counters_check(&begin, inp.words)
}

fn failure(i: usize, r: &MintRec, what: &str) -> String {
    format!("snap image: mint replay: record {i} (id {}): {what}", r.id)
}

fn mint_one(i: usize, r: &MintRec, inp: &Inputs, protos: &mut Vec<u64>) -> Result<(), String> {
    let fail = |what: String| failure(i, r, &what);
    if sh::snap_image_id_present(r.id) {
        return Err(fail("its id is present before the replay".into()));
    }
    if r.route != MINT_ROUTE_STATIC && !sh::snap_image_set_id_next(r.id) {
        return Err(fail("its id has no counter".into()));
    }
    if r.generation != 0 && (r.route == MINT_ROUTE_RUNTIME || r.route == MINT_ROUTE_LAZY) {
        sh::snap_image_set_semantic_next(r.generation);
    }
    let refused = || fail("the mint refuses its facts".into());
    let word = match r.route {
        MINT_ROUTE_DYNAMIC => {
            sh::snap_image_mint_dynamic(r, (inp.keys_at)(i, r), None).ok_or_else(refused)?
        }
        MINT_ROUTE_STATIC => {
            let id = Some(crate::codegen_abi::ShapeId(r.id));
            sh::snap_image_mint_dynamic(r, (inp.keys_at)(i, r), id).ok_or_else(refused)?
        }
        MINT_ROUTE_RUNTIME => {
            let off = r
                .desc_off
                .ok_or_else(|| fail("it names no descriptor".into()))?;
            let desc = inp.data_lo.wrapping_add(off as usize) as *const ld::PerryLayoutDesc;
            // SAFETY: the descriptor the capture found in the data window of this program.
            unsafe { ld::snap_image_replay_birth(desc, false, r.id, r.fam_serial) }.map_err(fail)?
        }
        MINT_ROUTE_LAZY => {
            let desc = ld::snap_image_lazy_descriptor(r.word);
            if desc == 0 {
                return Err(fail("its lazy row has no descriptor".into()));
            }
            // SAFETY: the program's lazy row descriptor of the record's word.
            unsafe { ld::snap_image_replay_birth(desc as *const _, true, r.id, r.fam_serial) }
                .map_err(fail)?
        }
        x => return Err(fail(format!("route {x} is unknown"))),
    };
    if word != r.word {
        return Err(fail(format!(
            "replayed as word {word:#x}, the record holds {:#x}",
            r.word
        )));
    }
    let pw = (inp.proto_word)(i, r);
    if pw != 0 && !protos.contains(&r.proto_id) {
        protos.push(r.proto_id);
        sh::snap_image_write_identity_word(r.proto_id, pw);
    }
    Ok(())
}

fn fill_handles(i: usize, r: &MintRec) -> Result<(), String> {
    let fail = |what: String| failure(i, r, &what);
    for t in r.handles {
        if t != 0 && sh::snap_image_mint_of_word(t).is_none() {
            return Err(fail(format!("its handle target {t:#x} is not minted")));
        }
    }
    if r.edges.iter().any(|e| *e != 0) {
        return Err(fail(
            "its rep deprecation edges have no replay setter".into(),
        ));
    }
    if !sh::snap_image_set_handles(r.word, r.handles) {
        return Err(fail("its record is gone".into()));
    }
    let back = sh::snap_image_mint_of_word(r.word).map(|m| m.handles);
    if back != Some(r.handles) {
        return Err(fail(format!(
            "handles {back:?} after the replay, the record holds {:?}",
            r.handles
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

    fn values(c: &[Counter]) -> Vec<u64> {
        c.iter().map(|c| c.value).collect()
    }

    /// Raise every counter to the largest value any run saw: the process keeps drawing fresh ids.
    fn settle(snaps: &[Vec<u64>]) {
        for (k, c) in counters_now().iter().enumerate() {
            let top = snaps
                .iter()
                .filter_map(|s| s.get(k).copied())
                .max()
                .unwrap_or(0);
            unsafe {
                if c.width == 4 {
                    AtomicU32::from_ptr(c.addr as *mut u32).fetch_max(top as u32, Relaxed);
                } else {
                    AtomicU64::from_ptr(c.addr as *mut u64).fetch_max(top, Relaxed);
                }
            }
        }
    }

    fn chunk_counter() -> Counter {
        counters_now()
            .into_iter()
            .find(|c| c.name == "shape chunk next")
            .expect("chunk counter")
    }

    /// A fresh agent; `chunk` positions the chunk counter before its runtime state exists, so its
    /// rows repeat those of the agent that started at that chunk.
    fn agent<T: Send + 'static>(chunk: Option<u64>, f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(move || {
            if let Some(k) = chunk {
                unsafe {
                    AtomicU32::from_ptr(chunk_counter().addr as *mut u32).store(k as u32, Relaxed)
                };
            }
            crate::test_support::bootstrap_test_thread();
            f()
        })
        .join()
        .expect("agent")
    }

    /// The words of the counters a recording moved, as the image's changed data words hold them.
    fn moved_words(before: &[Counter], after: &[Counter]) -> Vec<(usize, u64)> {
        let mut ats: Vec<usize> = Vec::new();
        for (b, a) in before.iter().zip(after) {
            if a.value != b.value && !ats.contains(&(a.addr & !7)) {
                ats.push(a.addr & !7);
            }
        }
        ats.into_iter()
            .map(|at| {
                let w = after
                    .iter()
                    .filter(|c| c.addr & !7 == at)
                    .fold(0u64, |w, c| w | c.value << ((c.addr - at) * 8));
                (at, w)
            })
            .collect()
    }

    fn mint(keys: usize, n: u32) -> u32 {
        sh::shape_descriptor_ensure(keys as *const crate::array::ArrayHeader, n, n)
            .expect("mint")
            .0
    }

    /// Recording agent: a draw, one wasted id, a second draw (a gap of one in the counter).
    fn record_two() -> (u64, Vec<MintRec>, Vec<(usize, u64)>, Vec<u64>) {
        let chunk = chunk_counter().value;
        let (recs, words, after) = agent(None, || {
            let before = counters_now();
            let a = mint(0x8067_0000_0051_0100, 1);
            sh::snap_image_test_waste_id(sh::PROTO_ID_DEFAULT);
            let b = mint(0x8067_0000_0051_0200, 2);
            let after = counters_now();
            let recs = vec![
                sh::snap_image_mint_of_word(a).expect("a"),
                sh::snap_image_mint_of_word(b).expect("b"),
            ];
            (recs, moved_words(&before, &after), values(&after))
        });
        (chunk, recs, words, after)
    }

    fn fresh_keys(i: usize, _: &MintRec) -> usize {
        0x8067_0000_0052_0100 + 0x100 * i
    }

    fn replay(recs: &[MintRec], words: &[(usize, u64)]) -> Result<(), String> {
        snap_image_mint_replay(
            recs,
            &Inputs {
                keys_at: &fresh_keys,
                proto_word: &|_, _| 0,
                data_lo: 0,
                words,
            },
        )
    }

    #[test]
    fn replay_positions_the_counter_over_a_gap() {
        let (chunk, recs, words, after) = record_two();
        assert!(
            recs.iter().all(|r| r.route == MINT_ROUTE_DYNAMIC),
            "dynamic records"
        );
        assert!(recs[1].id > recs[0].id + 1, "the wasted id leaves a gap");
        let r2 = recs.clone();
        let (res, ids, now) = agent(Some(chunk), move || {
            let res = replay(&r2, &words);
            let ids: Vec<Option<u32>> = r2
                .iter()
                .map(|r| sh::snap_image_mint_of_word(r.word).map(|m| m.id))
                .collect();
            (res, ids, values(&counters_now()))
        });
        settle(&[after, now]);
        assert_eq!(res, Ok(()));
        assert_eq!(ids, vec![Some(recs[0].id), Some(recs[1].id)]);
    }

    #[test]
    fn replay_fills_edges_after_all_mints() {
        let (chunk, mut recs, words, after) = record_two();
        // An edge from the first record to the second: the target is minted after the source.
        recs[0].handles[2] = recs[1].word;
        let r2 = recs.clone();
        let (res, back, now) = agent(Some(chunk), move || {
            let res = replay(&r2, &words);
            (
                res,
                sh::snap_image_mint_of_word(r2[0].word).map(|m| m.handles[2]),
                values(&counters_now()),
            )
        });
        settle(&[after, now]);
        assert_eq!(res, Ok(()));
        assert_eq!(back, Some(recs[1].word));
    }

    #[test]
    fn replay_refuses_a_present_id() {
        let (chunk, recs, words, after) = record_two();
        let (first, second, now) = agent(Some(chunk), move || {
            let first = replay(&recs, &words);
            (first, replay(&recs, &words), values(&counters_now()))
        });
        settle(&[after, now]);
        assert_eq!(first, Ok(()));
        let e = second.expect_err("a present id is refused");
        assert!(
            e.contains("record 0") && e.contains("present before the replay"),
            "{e}"
        );
    }

    #[test]
    fn replay_refuses_a_mint_that_gives_another_word() {
        let (chunk, mut recs, words, after) = record_two();
        recs[0].word = recs[1].word;
        let (res, now) = agent(Some(chunk), move || {
            (replay(&recs, &words), values(&counters_now()))
        });
        settle(&[after, now]);
        let e = res.expect_err("a record whose mint gives another word is refused");
        assert!(
            e.contains("record 0") && e.contains("replayed as word"),
            "{e}"
        );
    }

    #[test]
    fn filling_handles_refuses_a_handle_the_record_does_not_hold() {
        let (res, now) = agent(None, || {
            let a = mint(0x8067_0000_0053_0100, 1);
            let b = mint(0x8067_0000_0053_0200, 2);
            assert!(sh::snap_image_set_handles(a, [0, b, 0, 0, 0, 0]));
            let mut r = sh::snap_image_mint_of_word(a).expect("a");
            r.handles = [0; 6];
            (fill_handles(0, &r), values(&counters_now()))
        });
        settle(&[now]);
        let e = res.expect_err("a stale live handle is refused");
        assert!(e.contains("after the replay"), "{e}");
    }

    #[test]
    fn replay_positions_the_prototype_serial() {
        let chunk = chunk_counter().value;
        let (rec, words, after) = agent(None, || unsafe {
            let d = ld::snap_image_test_family_layout();
            let before = counters_now();
            let word = ld::layout_word_for_birth(d).expect("a runtime layout installs");
            let after = counters_now();
            let mut r = sh::snap_image_mint_of_word(word).expect("installed");
            r.fam_serial = (*(*d).proto).serial;
            (r, moved_words(&before, &after), values(&after))
        });
        assert_eq!(rec.route, MINT_ROUTE_RUNTIME);
        assert_ne!(rec.fam_serial, 0, "the first birth draws the family serial");
        let r2 = rec.clone();
        let (res, cell, serial, now) = agent(Some(chunk), move || unsafe {
            let d = ld::snap_image_test_family_layout();
            let mut r = r2;
            r.desc_off = Some(d as u64);
            let res = replay(std::slice::from_ref(&r), &words);
            (
                res,
                (*d).shape_id,
                (*(*d).proto).serial,
                values(&counters_now()),
            )
        });
        settle(&[after, now]);
        assert_eq!(res, Ok(()));
        assert_eq!(cell, rec.id);
        assert_eq!(serial, rec.fam_serial);
    }

    #[test]
    fn replay_asserts_the_counters_against_the_word_list() {
        let (chunk, recs, words, after) = record_two();
        let mut wrong = words.clone();
        let id_word = recs[1].id;
        // The word of the counter the second record drew from, one id further.
        let k = counters_now()
            .iter()
            .filter(|c| c.width == 4 && c.name.starts_with("shape id next"))
            .find_map(|c| {
                wrong
                    .iter()
                    .position(|w| {
                        w.0 == c.addr & !7
                            && counter_in_words(c, &words) == Some(u64::from(id_word) + 1)
                    })
                    .map(|p| (p, c.addr))
            })
            .expect("the drawn counter is in the word list");
        wrong[k.0].1 += 1u64 << ((k.1 & 7) * 8);
        let (r1, r2) = (recs.clone(), recs);
        let (good, now1) = agent(Some(chunk), move || {
            (replay(&r1, &words), values(&counters_now()))
        });
        let (bad, now2) = agent(Some(chunk), move || {
            (replay(&r2, &wrong), values(&counters_now()))
        });
        settle(&[after, now1, now2]);
        assert_eq!(good, Ok(()));
        let e = bad.expect_err("a counter off the word list");
        assert!(
            e.contains("counter shape id next") && e.contains("differs"),
            "{e}"
        );
    }
}
