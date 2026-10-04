"""Read-only 64-byte ShapeRecord census for exact ebc858 main on Linux x86-64.
Loaded into GDB before meta-sharing-observer.py. Validate counts against the
runtime's own census; these are storage bytes, not an RSS counterfactual.
"""
import collections, gdb, os, struct

def observe_shapes(read):
    def address(env):
        return int(gdb.parse_and_eval("(unsigned long)&'" + os.environ[env] + "'"))
    directory = address('SHAPE_DIR_SYMBOL')
    empty_page = address('SHAPE_EMPTY_PAGE_SYMBOL')
    empty_chunk = address('SHAPE_EMPTY_CHUNK_SYMBOL')
    pages_seen = set(); chunks_seen = set(); extras_seen = set(); spans = []
    count = 0; extra_count = 0; body_entries = 0; body_histogram = collections.Counter()
    directory_used_bytes = 0; records_by_kind = collections.Counter()
    for band in range(3):
        pages, length, base_rel = struct.unpack('<QQI', read(directory + band * 32, 20))
        assert length < 1_000_000
        directory_used_bytes += length * 8
        if length:
            spans.append([pages, length * 8, 'shape directory used slots'])
        for (page,) in struct.iter_unpack('<Q', read(pages, length * 8)):
            if page == empty_page:
                continue
            assert page and page not in pages_seen
            pages_seen.add(page); spans.append([page, 8192, 'shape directory page'])
            for (chunk,) in struct.iter_unpack('<Q', read(page, 8192)):
                if chunk == empty_chunk:
                    continue
                assert chunk and chunk not in chunks_seen
                chunks_seen.add(chunk); spans.append([chunk, 2048, 'shape record chunk'])
                for i in range(32):
                    record = read(chunk + i * 64, 64)
                    flags, = struct.unpack_from('<I', record, 36)
                    if not flags & 1:
                        continue
                    count += 1; records_by_kind[(flags >> 8) & 7] += 1
                    mask, = struct.unpack_from('<I', record, 44)
                    extra, = struct.unpack_from('<Q', record, 56)
                    if not extra:
                        assert mask == 0
                        continue
                    assert extra not in extras_seen
                    extras_seen.add(extra)
                    ptr, length = struct.unpack('<QQ', read(extra, 16))
                    assert 0 < length <= 32 and length == mask.bit_count(), (length, mask)
                    actual_mask = 0
                    previous = -1
                    for j in range(length):
                        entry = read(ptr + j * 16, 16)
                        # Pinned nightly-2026-08-20 x86_64 layout: info@0,
                        # slot@8, size16. Verified with the exact source type
                        # in shape-constfn-layout-probe.rs and runtime preflight.
                        # Never interpret the seven padding bytes as an info
                        # pointer: that heuristic can be ambiguous on real heaps.
                        slot = entry[8]
                        info, = struct.unpack_from('<Q', entry, 0)
                        assert previous < slot < 32 and mask & (1 << slot) and info > 4096, (slot, info, mask)
                        previous = slot; actual_mask |= 1 << slot
                    assert actual_mask == mask
                    extra_count += 1; body_entries += length; body_histogram[length] += 1
                    spans.extend([[extra, 24, 'shape extras'], [ptr, length * 16, 'ConstFn body lists']])
    return dict(records=count, records_by_kind=dict(records_by_kind), chunks=len(chunks_seen),
                pages=len(pages_seen), directory_used_bytes=directory_used_bytes,
                chunk_storage_bytes=len(chunks_seen) * 2048,
                present_record_bytes=count * 64,
                empty_record_slot_bytes=(len(chunks_seen) * 32 - count) * 64,
                directory_page_bytes=len(pages_seen) * 8192,
                extras_count=extra_count, extras_payload_bytes=extra_count * 24,
                constfn_entries=body_entries, constfn_list_payload_bytes=body_entries * 16,
                constfn_list_lengths=dict(body_histogram), spans=spans,
                record_growth_payload_bytes=count * 8,
                record_growth_chunk_bytes=len(chunks_seen) * 32 * 8)
