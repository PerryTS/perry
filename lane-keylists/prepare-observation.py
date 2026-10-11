#!/usr/bin/env python3
"""Prepare identical private observation code in both A/B arms.

Existing GC timers omit synchronous automatic fulls, and js_gc_stats is
thread-local. Count every collector's completion and active pause time here.
These four integer counters are measurement code, never a shipped runtime mode.
"""
import argparse,shutil,subprocess,os
from pathlib import Path
ROOT=Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
p=argparse.ArgumentParser();p.add_argument('arm',choices=['main','head']);p.add_argument('--reuse',action='store_true');p.add_argument('--refresh',action='store_true');a=p.parse_args()
original=ROOT if a.arm=='head' else ROOT/os.environ.get('KEYLISTS_MAIN', 'main')
base=ROOT/('obs-'+a.arm)
if a.refresh:
    assert (base/'target').is_dir(),base
    subprocess.run(['rsync', '-a', '--delete', '--exclude', 'target', str(original/'src')+'/', str(base/'src')+'/'], check=True)
elif a.reuse:
    assert (base/'target').is_dir(),base
    shutil.rmtree(base/'src')
else:
    assert not (base/'target').exists(),base
    if base.exists(): shutil.rmtree(base)
    base.mkdir()
if not a.refresh:
    shutil.copytree(original/'src',base/'src',ignore=shutil.ignore_patterns('.git','target','.perry-trace','__pycache__'))
# Clone only the same arm's build cache; main and head remain independent.
if not (a.reuse or a.refresh):
    subprocess.run(['cp','-a','--reflink=auto',str(original/'target'),str(base/'target')],check=True)
gc=base/'src/crates/perry-runtime/src/gc'
f=gc/'mod.rs';s=f.read_text();s=s.replace('pub(crate) mod instruments;','pub(crate) mod instruments;\nmod lane_observe;',1)
needle='.record_collection(freed_bytes, elapsed_us);\n        });'
assert needle in s
s=s.replace(needle,needle+'\n        lane_observe::record(true, elapsed_us);',1);f.write_text(s)
f=gc/'cycle.rs';s=f.read_text();needle='.record_collection(self.freed_bytes, elapsed_us);\n        });';assert needle in s
s=s.replace(needle,needle+'\n        super::lane_observe::record(self.minor.is_some(), elapsed_us);',1);f.write_text(s)
(gc/'lane_observe.rs').write_text('''use std::sync::atomic::{AtomicU64,Ordering};
#[no_mangle] pub static PERRY_LANE_FULL_COUNT: AtomicU64 = AtomicU64::new(0);
#[no_mangle] pub static PERRY_LANE_MINOR_COUNT: AtomicU64 = AtomicU64::new(0);
#[no_mangle] pub static PERRY_LANE_FULL_PAUSE_US: AtomicU64 = AtomicU64::new(0);
#[no_mangle] pub static PERRY_LANE_MINOR_PAUSE_US: AtomicU64 = AtomicU64::new(0);
#[inline] pub(super) fn record(minor: bool, us: u64) {
    let (count,pause)=if minor {(&PERRY_LANE_MINOR_COUNT,&PERRY_LANE_MINOR_PAUSE_US)}
                     else {(&PERRY_LANE_FULL_COUNT,&PERRY_LANE_FULL_PAUSE_US)};
    count.fetch_add(1,Ordering::Relaxed);
    pause.fetch_add(us,Ordering::Relaxed);
}
''')
print(base,flush=True)
