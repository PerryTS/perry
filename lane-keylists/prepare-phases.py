#!/usr/bin/env python3
"""Add private per-phase retired-instruction counters to an observation arm."""
import argparse
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('source', type=Path)
a = p.parse_args()
gc = a.source / 'crates/perry-runtime/src/gc'
f = gc / 'lane_observe.rs'
f.write_text(f.read_text() + Path(__file__).with_name('phase-observe.rs').read_text().replace('use std::sync::atomic::{AtomicU64, Ordering};', ''))
f = gc / 'cycle.rs'
s = f.read_text()
needle = 'let step_start = Instant::now();'
assert needle in s
s = s.replace(needle, '''let _lane_phase = super::lane_observe::Phase::enter(match self.phase {
            GcCyclePhase::BuildValidPointerSet => 0,
            GcCyclePhase::RootScan => 1,
            GcCyclePhase::MarkPropagation => 2,
            GcCyclePhase::BlockPersistence => 3,
            GcCyclePhase::AtomicFinalize => 4,
            GcCyclePhase::Sweep => 5,
            GcCyclePhase::Reclaim => 6,
            GcCyclePhase::Complete => 7,
        });
        ''' + needle, 1)
f.write_text(s)
f = gc / 'mod.rs'
s = f.read_text()
needle = 'let mut trace = GcCycleTrace::new(GcCollectionKind::Minor, trigger);'
assert needle in s
s = s.replace(needle, needle + '\n    let _lane_phase = lane_observe::Phase::enter(8);', 1)
f.write_text(s)
f = a.source / 'crates/perry-runtime/src/array/alloc.rs'
s = f.read_text()
needle = 'let bytes = array_byte_size(capacity as usize + reserve);'
if needle in s:
    s = s.replace(needle, needle + '\n    crate::gc::lane_observe::birth(bytes);\n    let _lane_phase = crate::gc::lane_observe::Phase::enter(9);', 1)
else:
    for name, size in [('js_array_alloc_key_list', 'capacity as usize'), ('js_array_alloc_key_list_reserved', 'capacity as usize + reserve')]:
        start = s.index('pub(crate) fn ' + name + '(')
        at = s.index('    let ptr = arena_alloc_gc(', start)
        s = s[:at] + '    crate::gc::lane_observe::birth(array_byte_size(' + size + '));\n    let _lane_phase = crate::gc::lane_observe::Phase::enter(9);\n' + s[at:]
f.write_text(s)
f = gc / 'mod.rs'
f.write_text(f.read_text().replace('mod lane_observe;', 'pub(crate) mod lane_observe;', 1))
f = gc / 'lane_observe.rs'
f.write_text(f.read_text().replace('pub(super)', 'pub(crate)'))
