#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-runtime-currente532-v12-build.exit"' EXIT
test "$(cat "$B/gc-runtime-main69r1-auto.exit")" = 0
test "$(cat "$B/gc-runtime-window-v6r1-auto.exit")" = 0
export PATH=/root/.cargo/bin:$PATH CARGO_BUILD_JOBS=4 CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_INCREMENTAL=0 LLVM_SYS_221_PREFIX=/usr/lib/llvm-22 RUST_TEST_THREADS=1
python3 - <<'SETUP'
from pathlib import Path
import hashlib,json,shutil,subprocess,tarfile,time
B=Path('/root/rss-header-20261002')
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
archives=json.loads((B/'currente532-source-archives.json').read_text())
assert all(sha(B/n)==h for n,h in archives.items())
old=json.loads((B/'gc-runtime-main69-products.json').read_text())
assert all(sha(p)==h for p,h in old.items()), 'old products changed'
new=json.loads((B/'provenance-gc-runtime-currente532-v12.json').read_text())
assert new['base']=='e532bf3d0efce80ef699aa67b4834967050cf5ef'
assert new['archive_sha256']==archives['gc-runtime-currente532-v12-source.tar.gz']
copies=[(B/'main69-base-target',B/'currente532-base-target'),
        (B/'main69-gc-target',B/'currente532-gc-target')]
needed=sum(int(subprocess.check_output(['du','-sb',str(src)]).split()[0]) for src,_ in copies)
assert shutil.disk_usage(B).free>needed+12*2**30
for src,dst in copies:
    assert not dst.exists(), 'preserve existing target evidence'
    subprocess.run(['cp','-a','--reflink=auto',str(src),str(dst)],check=True)
refresh={}
for arm in ['base','gc']:
    root=B/('perry-currente532-'+arm)
    assert not root.exists(), 'preserve existing source evidence'
    root.mkdir()
    with tarfile.open(B/'perry-currente532-source.tar.gz') as t:
        t.extractall(root,filter='data')
    if arm=='gc':
        with tarfile.open(B/'gc-runtime-currente532-v12-source.tar.gz') as t:
            t.extractall(root,filter='data')
        assert all(sha(root/p)==h for p,h in new['files'].items())
    # A new main changes compiler/HIR/ABI too. Refresh the whole workspace,
    # not only runtime inputs: archived Git mtimes can predate reused targets.
    touched=[]
    for p in root.rglob('*'):
        if p.is_file() and not p.is_symlink() and (p.suffix=='.rs' or p.name in ['Cargo.toml','Cargo.lock']):
            p.touch();touched.append(str(p.relative_to(root)))
    assert 'crates/perry-codegen/src/lib.rs' in touched
    assert 'crates/perry-abi/src/lib.rs' in touched
    assert 'crates/perry-runtime/src/lib.rs' in touched
    refresh[arm]=touched
(B/'gc-runtime-currente532-v12-workspace-refresh.json').write_text(json.dumps(dict(archives=archives,refreshed=refresh),indent=2)+'\n')
start=time.monotonic()
while int(next(s.split()[1] for s in open('/proc/meminfo') if s.startswith('MemAvailable:')))<18*2**20:
    assert time.monotonic()-start<7200,'compile headroom timeout'
    time.sleep(30)
SETUP
cd "$B/perry-currente532-base"
export CARGO_TARGET_DIR=$B/currente532-base-target
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static > "$B/logs/currente532-base-build.log" 2>&1
mkdir "$B/currente532-base"
cp "$CARGO_TARGET_DIR/release/perry" "$CARGO_TARGET_DIR/release/libperry_runtime.a" "$CARGO_TARGET_DIR/release/libperry_stdlib.a" "$B/currente532-base/"
cd "$B/perry-currente532-gc"
export CARGO_TARGET_DIR=$B/currente532-gc-target
cargo test --release -p perry-runtime --lib > "$B/logs/currente532-gc-runtime-tests.log" 2>&1
cargo test --release -p perry-ffi --features runtime-link --lib > "$B/logs/currente532-gc-ffi-tests.log" 2>&1
cargo test --release -p perry-ext-events --lib > "$B/logs/currente532-gc-events-tests.log" 2>&1
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static > "$B/logs/currente532-gc-build.log" 2>&1
mkdir "$B/currente532-gc"
cp "$CARGO_TARGET_DIR/release/perry" "$CARGO_TARGET_DIR/release/libperry_runtime.a" "$CARGO_TARGET_DIR/release/libperry_stdlib.a" "$B/currente532-gc/"
python3 "$B/verify-gc-runtime-build.py" "$B/logs/currente532-gc-runtime-tests.log" "$B/currente532-gc/libperry_runtime.a" "$B/currente532-base/libperry_runtime.a" "$B/gc-runtime-currente532-v12-artifact-verification.json" --pages
python3 - <<'WITNESS'
from pathlib import Path
import hashlib,json,re,subprocess
B=Path('/root/rss-header-20261002');sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
testlog=(B/'logs/currente532-gc-runtime-tests.log').read_text()
required=['reused_blocks_restart_the_window_and_cold_blocks_are_advised_once',
          'real_collection_publication_keeps_warm_pages_and_releases_unused_pages',
          'a_present_key_is_answered_by_the_receivers_shape',
          'arguments_objects_of_one_arity_share_one_shape',
          'construction_is_born_from_the_prototype_birth_record',
          'later_evaluations_are_born_in_the_template_shapes']
for n in required:assert re.search(r'^test .*::'+n+r' \.\.\. ok$',testlog,re.M),n
for arm in ['base','gc']:
    log=(B/'logs'/('currente532-'+arm+'-build.log')).read_text()
    assert re.search(r'^\s*Compiling perry-codegen ',log,re.M), 'compiler not rebuilt'
    symbols=subprocess.check_output(['nm','--defined-only',B/('currente532-'+arm)/'libperry_runtime.a'],stderr=subprocess.DEVNULL)
    assert b'js_class_object_set_ctor_caps' in symbols, 'fresh-class lane missing'
    assert b'js_typed_feedback_object_get_field_by_key_f64' in symbols, 'maine532 runtime lane missing'
    assert b'visit_gc_rewrite_slot_descriptors_inline' not in symbols, 'rejected specialization remains'
    assert sha(B/('currente532-'+arm)/'perry')!=sha(B/('main69-'+arm)/'perry'), 'maine532 compiler identical to old compiler'
    if arm=='gc':assert b'advance_block_pool_reuse_window' in symbols, 'pool window missing'
products={str(p):sha(p) for arm in ['currente532-base','currente532-gc'] for p in (B/arm).iterdir() if p.is_file()}
(B/'gc-runtime-currente532-v12-products.json').write_text(json.dumps(products,indent=2)+'\n')
(B/'gc-runtime-currente532-v12-policy-verification.json').write_text(json.dumps(dict(required_passed=required,compiler_rebuilt_both_arms=True,rejected_helper_absent=True,current_dynamic_key_symbol=True),indent=2)+'\n')
WITNESS
