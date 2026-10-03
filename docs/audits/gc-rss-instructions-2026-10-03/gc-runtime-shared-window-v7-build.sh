#!/usr/bin/env bash
set -euo pipefail
B=/root/rss-header-20261002
trap 'echo $? > "$B/gc-runtime-shared-window-v7-build.exit"' EXIT
test "$(cat "$B/gc-runtime-main69r1-auto.exit")" = 0
test "$(cat "$B/gc-runtime-window-v6-auto.exit")" = 0
export PATH=/root/.cargo/bin:$PATH CARGO_BUILD_JOBS=4 CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_INCREMENTAL=0 LLVM_SYS_221_PREFIX=/usr/lib/llvm-22 RUST_TEST_THREADS=1
python3 - <<'SETUP'
from pathlib import Path
import hashlib,json,subprocess,shutil
B=Path('/root/rss-header-20261002');sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
old=json.loads((B/'provenance-gc-runtime-pages-main69.json').read_text())
assert all(sha(B/'perry-main69-gc'/p)==h for p,h in old['files'].items())
products=json.loads((B/'gc-runtime-main69-products.json').read_text())
assert all(sha(p)==h for p,h in products.items())
new=json.loads((B/'provenance-gc-runtime-shared-window-v7.json').read_text())
assert sha(B/'gc-runtime-shared-window-v7-source.tar.gz')==new['archive_sha256']
source=B/'perry-main69-shared'; target=B/'main69-shared-target'
assert not source.exists() and not target.exists(), 'preserve existing build evidence'
copies=[(B/'perry-main69-gc',source),(B/'main69-gc-target',target)]
needed=sum(int(subprocess.check_output(['du','-sb',str(src)]).split()[0]) for src,_ in copies)
assert shutil.disk_usage(B).free > needed+12*2**30, 'need space for independent source/target copies'
for src,dst in copies:
    subprocess.run(['cp','-a','--reflink=auto',str(src),str(dst)],check=True)
(B/'gc-runtime-shared-window-v7-input-copy.json').write_text(json.dumps(dict(original_products=products,source_overlay_sha256=new['archive_sha256'],copied_source=str(source),independent_target=str(target)),indent=2)+'\n')
SETUP
python3 "$B/prepare-gc-overlay.py" "$B/perry-main69-shared" "$B/gc-runtime-shared-window-v7-source.tar.gz" "$B/provenance-gc-runtime-shared-window-v7.json" "$B/gc-runtime-shared-window-v7-source-refresh.json"
cd "$B/perry-main69-shared"
export CARGO_TARGET_DIR=$B/main69-shared-target
python3 - <<'HEADROOM'
import time
start=time.monotonic()
while int(next(s.split()[1] for s in open('/proc/meminfo') if s.startswith('MemAvailable:')))<18*2**20:
    assert time.monotonic()-start<7200,'compile headroom timeout'
    time.sleep(30)
HEADROOM
cargo test --release -p perry-runtime --lib > "$B/logs/gc-runtime-shared-window-v7-linux-tests.log" 2>&1
cargo test --release -p perry-ffi --features runtime-link --lib > "$B/logs/gc-runtime-shared-window-v7-linux-ffi.log" 2>&1
cargo test --release -p perry-ext-events --lib > "$B/logs/gc-runtime-shared-window-v7-linux-events.log" 2>&1
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static > "$B/logs/gc-runtime-shared-window-v7-linux-build.log" 2>&1
mkdir "$B/main69-shared"
cp "$CARGO_TARGET_DIR/release/perry" "$CARGO_TARGET_DIR/release/libperry_runtime.a" "$CARGO_TARGET_DIR/release/libperry_stdlib.a" "$B/main69-shared/"
python3 "$B/verify-gc-runtime-build.py" "$B/logs/gc-runtime-shared-window-v7-linux-tests.log" "$B/main69-shared/libperry_runtime.a" "$B/main69-gc/libperry_runtime.a" "$B/gc-runtime-shared-window-v7-artifact-verification.json" --pages
python3 - <<'WITNESS'
from pathlib import Path
import re,json,hashlib,subprocess
B=Path('/root/rss-header-20261002');text=(B/'logs/gc-runtime-shared-window-v7-linux-tests.log').read_text()
required=['reused_blocks_restart_the_window_and_cold_blocks_are_advised_once','real_collection_publication_keeps_warm_pages_and_releases_unused_pages']
for name in required:assert re.search(r'^test .*::'+name+r' \.\.\. ok$',text,re.M),name
symbols=subprocess.check_output(['nm','--defined-only',B/'main69-shared/libperry_runtime.a'],stderr=subprocess.DEVNULL)
assert b'advance_block_pool_reuse_window' in symbols,'new publication code absent from production archive'
products={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (B/'main69-shared').iterdir() if p.is_file()}
(B/'gc-runtime-shared-window-v7-products.json').write_text(json.dumps(products,indent=2)+'\n')
(B/'gc-runtime-shared-window-v7-policy-verification.json').write_text(json.dumps(dict(passed_tests=required,reuse_window_symbol=True,symbol_listing_sha256=hashlib.sha256(symbols).hexdigest()),indent=2)+'\n')
WITNESS
