use super::*;
use std::sync::mpsc;

#[test]
fn cross_thread_byte_cell_and_fake_family_word() {
    let (send, recv) = mpsc::channel();
    let (done_send, done_recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        crate::agent::enter_worker_agent();
        let addr = crate::buffer::buffer_alloc(24) as usize;
        send.send((addr, crate::agent::current_agent())).unwrap();
        done_recv.recv().unwrap();
    });
    let (addr, owner) = recv.recv().unwrap();
    let snapshot = classify(addr).expect("LIVE SUBJECT: remote mapped byte cell");
    assert_eq!(snapshot.owner, owner);
    assert!(!snapshot.is_current_thread());
    assert_eq!(snapshot.kind, 2);
    assert!(crate::buffer::header::byte_cell_is_owned(
        addr,
        crate::gc::GC_TYPE_BUFFER
    ));
    #[repr(C)]
    struct Fake {
        header: crate::gc::GcHeader,
        body: [u64; 3],
    }
    let fake = Box::new(Fake {
        header: crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_BUFFER,
            gc_flags: crate::gc::GC_FLAG_ARENA,
            _reserved: 0,
            size: std::mem::size_of::<Fake>() as u32,
        },
        body: [0; 3],
    });
    let fake_addr = fake.body.as_ptr() as usize;
    assert!(!contains(
        fake_addr - crate::gc::GC_HEADER_SIZE,
        crate::gc::GC_HEADER_SIZE
    ));
    assert!(!crate::buffer::header::byte_cell_is_owned(
        fake_addr,
        crate::gc::GC_TYPE_BUFFER
    ));
    done_send.send(()).unwrap();
    thread.join().unwrap();
}

#[test]
fn retire_and_same_address_reuse_changes_incarnation() {
    unsafe {
        let data = super::super::map(Kind::OldBlock, ALIGN);
        let first = classify(data as usize).expect("LIVE SUBJECT: mapping published");
        super::super::unmap(data, ALIGN);
        assert!(
            classify(data as usize).is_none(),
            "retired slot must reject"
        );
        let replacement = super::super::map(Kind::OldBlock, ALIGN);
        assert_eq!(replacement, data, "LIVE SUBJECT: exact slot reused");
        let second = classify(replacement as usize).unwrap();
        assert_ne!(
            first.incarnation, second.incarnation,
            "same-kind ABA must change token"
        );
        assert_eq!(second.owner, first.owner);
        assert!(!first.is_current(data as usize));
        assert!(second.is_current(data as usize));
        super::super::unmap(replacement, ALIGN);
    }
}

#[test]
fn concurrent_classifier_declines_snapshot_crossing_retirement() {
    unsafe {
        let data = super::super::map(Kind::OldBlock, ALIGN);
        assert!(
            contains(data as usize, ALIGN),
            "LIVE SUBJECT: published mapping"
        );
        let address = data as usize;
        let (entered_send, entered_recv) = mpsc::channel();
        let (continue_send, continue_recv) = mpsc::channel();
        let classifier = std::thread::spawn(move || {
            classify_between(address, || {
                entered_send.send(()).unwrap();
                continue_recv.recv().unwrap();
            })
        });
        entered_recv.recv().unwrap();
        super::super::unmap(data, ALIGN);
        let replacement = super::super::map(Kind::NurseryBlock, ALIGN);
        assert_eq!(data, replacement);
        continue_send.send(()).unwrap();
        assert!(
            classifier.join().unwrap().is_none(),
            "mixed old/new snapshot must reject"
        );
        assert_eq!(classify(address).unwrap().kind, 1);
        super::super::unmap(replacement, ALIGN);
    }
}

#[test]
fn arbitrary_words_unused_slots_and_extent_boundaries_are_safe() {
    unsafe {
        let data = super::super::map(Kind::LargeObject, 2 * ALIGN);
        assert!(!data.is_null());
        for address in [
            0,
            1,
            usize::MAX,
            existing().unwrap().descriptors,
            existing().unwrap().base + existing().unwrap().len - ALIGN,
        ] {
            assert!(classify(address).is_none());
        }
        note_payload_cell(data as usize + ALIGN);
        assert!(classify(data as usize).unwrap().payload_cells);
        assert!(classify(data as usize + ALIGN).unwrap().payload_cells);
        assert!(contains(data as usize + ALIGN, ALIGN));
        assert!(!contains(data as usize + ALIGN, ALIGN + 1));
        assert!(!contains(usize::MAX, 1));
        super::super::unmap(data, 2 * ALIGN);
    }
}

#[test]
fn active_metadata_retags_and_retires_without_a_range_map() {
    unsafe {
        let data = super::super::map(Kind::NurseryBlock, ALIGN);
        let mut starts = [1u64];
        assert!(set_space(
            data as usize,
            ALIGN,
            HeapSpace::NurseryEden,
            Some(starts.as_mut_ptr() as usize)
        ));
        let first = classify(data as usize).unwrap();
        assert_eq!(first.generation(), HeapGeneration::Nursery);
        assert_eq!(first.starts, starts.as_ptr() as usize);
        note_payload_cell(data as usize);
        assert!(classify(data as usize).unwrap().payload_cells);
        assert!(set_space(
            data as usize,
            ALIGN,
            HeapSpace::PromotedYoung,
            None
        ));
        assert_eq!(
            classify(data as usize).unwrap().generation(),
            HeapGeneration::Old
        );
        assert!(set_space(data as usize, ALIGN, HeapSpace::Unknown, Some(0)));
        assert_eq!(classify(data as usize).unwrap().starts, 0);
        super::super::unmap(data, ALIGN);
    }
}

#[test]
fn reservation_sizing_obeys_finite_limits_and_slot_geometry() {
    assert_eq!(std::mem::size_of::<Descriptor>(), 128);
    assert_eq!(initial_payload_len(None), 1usize << 40);
    assert_eq!(initial_payload_len(Some(libc::RLIM_INFINITY)), 1usize << 40);
    assert_eq!(
        initial_payload_len(Some(512 * 1024 * 1024 + 7)),
        128 * 1024 * 1024
    );
    assert!(initial_payload_len(Some((MIN_PAYLOAD * 4 - 1) as _)) < MIN_PAYLOAD);
    assert_eq!(initial_payload_len(Some(0)), 0);
}

#[test]
fn owner_classifiers_project_without_remote_snapshots() {
    unsafe {
        let data = super::super::map(Kind::LargeObject, 2 * ALIGN);
        assert!(!data.is_null(), "LIVE SUBJECT: two-slot extent");
        let base = data as usize;
        let mut starts = [1u64];
        for (space, generation) in [
            (HeapSpace::NurseryEden, HeapGeneration::Nursery),
            (HeapSpace::Survivor0, HeapGeneration::Nursery),
            (HeapSpace::Survivor1, HeapGeneration::Nursery),
            (HeapSpace::Longlived, HeapGeneration::Longlived),
            (HeapSpace::Old, HeapGeneration::Old),
            (HeapSpace::PromotedYoung, HeapGeneration::Old),
        ] {
            assert!(set_space(
                base,
                2 * ALIGN,
                space,
                Some(starts.as_mut_ptr() as usize)
            ));
            SNAPSHOT_READS.store(0, SeqCst);
            for addr in [base, base + ALIGN + 8, base + 2 * ALIGN - 1] {
                assert_eq!(
                    crate::arena::page_meta::classify_heap_generation(addr),
                    generation
                );
                assert_eq!(
                    crate::arena::page_meta::classify_heap_space_in_range(addr),
                    Some((space, base, starts.as_mut_ptr()))
                );
            }
            assert_eq!(
                crate::arena::page_meta::uniform_heap_generation(base + 8, base + 2 * ALIGN),
                Some(generation)
            );
            assert_eq!(
                SNAPSHOT_READS.load(SeqCst),
                0,
                "owner classification must not copy a remote snapshot"
            );
            assert!(
                classify(base).is_some(),
                "LIVE SUBJECT: remote snapshot remains available"
            );
            assert_eq!(SNAPSHOT_READS.load(SeqCst), 1);
            let remote = std::thread::spawn(move || {
                assert!(classify(base).is_some());
                assert_eq!(owned_generation(base), None);
                assert_eq!(owned_space(base), None);
                assert_eq!(owned_uniform_generation(base, base + ALIGN), None);
                assert_eq!(
                    crate::arena::page_meta::classify_heap_generation(base),
                    HeapGeneration::Unknown
                );
                assert_eq!(
                    crate::arena::page_meta::classify_heap_space_in_range(base),
                    None
                );
            });
            remote.join().unwrap();
        }
        assert_eq!(owned_uniform_generation(base, base), None);
        assert_eq!(owned_uniform_generation(base, base + 2 * ALIGN + 1), None);
        assert!(set_space(base, 2 * ALIGN, HeapSpace::Unknown, Some(0)));
        assert_eq!(owned_generation(base), Some(HeapGeneration::Unknown));
        assert_eq!(owned_space(base), None);
        super::super::unmap(data, 2 * ALIGN);
        assert_eq!(owned_generation(base), None);
        assert_eq!(owned_space(base), None);
    }
}

#[test]
fn registering_thread_owns_a_block_mapped_by_another_thread() {
    // The from-space quarantine ring hands a retired block to whichever
    // thread evicts it. Registration on the new thread must move ownership.
    let data = std::thread::spawn(|| unsafe { super::super::map(Kind::NurseryBlock, ALIGN) as usize })
        .join()
        .unwrap();
    assert!(data != 0, "LIVE SUBJECT: block mapped by another thread");
    assert_eq!(owned_generation(data), None);
    let mut starts = [1u64];
    assert!(set_space(
        data,
        ALIGN,
        HeapSpace::NurseryEden,
        Some(starts.as_mut_ptr() as usize)
    ));
    assert_eq!(
        crate::arena::page_meta::classify_heap_generation(data + 8),
        HeapGeneration::Nursery
    );
    assert_eq!(
        crate::arena::page_meta::classify_heap_space_in_range(data + 8),
        Some((HeapSpace::NurseryEden, data, starts.as_mut_ptr()))
    );
    let snapshot = classify(data).unwrap();
    assert!(snapshot.is_current_thread());
    assert_eq!(snapshot.owner, crate::agent::current_agent());
    std::thread::spawn(move || assert_eq!(owned_generation(data), None))
        .join()
        .unwrap();
    assert!(set_space(data, ALIGN, HeapSpace::Unknown, Some(0)));
    unsafe { super::super::unmap(data as *mut u8, ALIGN) };
}
