"""Own isolated current-main baseline and GC proposal builds/tests.

Copies only an owned completed cache. Refreshes every tracked Rust/Cargo input;
package set, features, codegen units and toolchain are equal in both arms.
"""
import pathlib,json,hashlib,subprocess,os,tarfile,time,shutil,concurrent.futures
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-latest2026-v31';R.mkdir(exist_ok=False)
E=R/'export';E.mkdir()
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
proof=json.loads((B/'provenance-latest2026-v31.json').read_text())
assert sha(B/'perry-latest2026-v31-source.tar.gz')==proof['baseline_archive_sha256']
assert sha(B/'gc-runtime-latest2026-v31-overlay.tar.gz')==proof['overlay_archive_sha256']
for arm in ['base','gc']:
    s=R/('source-'+arm);s.mkdir();t=R/('target-'+arm)
    with tarfile.open(B/'perry-latest2026-v31-source.tar.gz') as a:a.extractall(s,filter='data')
    if arm=='gc':
        with tarfile.open(B/'gc-runtime-latest2026-v31-overlay.tar.gz') as a:a.extractall(s,filter='data')
    manifest=B/('latest2026-v31-'+arm+'-inputs.json');assert sha(manifest)==proof['manifest_hashes'][arm]
    inputs=json.loads(manifest.read_text());assert all(sha(s/n)==h for n,h in inputs.items())
    for n in inputs:(s/n).touch()
    save(arm+'-source-inputs.json',inputs)
    subprocess.run(['cp','-a','--reflink=auto',B/'primary-weak-v24-owned-cache',t],check=True)
save('provenance.json',proof|dict(host=subprocess.check_output(['hostname'],text=True).strip(),script_sha256=sha(__file__)))
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1'}
def build(arm):
    s=R/('source-'+arm);t=R/('target-'+arm);out=E/arm;out.mkdir();records=[]
    ae=env|{'CARGO_TARGET_DIR':str(t),'PERRY_RUNTIME_DIR':str(t/'release'),'PERRY_WORKSPACE_ROOT':str(s)}
    for name,cmd in [('build',['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']),('runtime-tests',['cargo','test','--locked','--release','-p','perry-runtime','--lib'])]:
        start=time.monotonic()
        with (out/(name+'.log')).open('w') as f:
            p=subprocess.Popen(cmd,cwd=s,env=ae,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
        records.append(dict(name=name,command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(out/(name+'.log'))));save(arm+'-commands.json',records)
        assert rc==0 and 'Compiling perry-runtime ' in (out/(name+'.log')).read_text(),records[-1]
        if name=='build':
            for n in ['perry','libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(t/'release'/n,out/n)
            save(arm+'-products.json',{n:sha(out/n) for n in ['perry','libperry_runtime.a','libperry_stdlib.a']})
        print(arm,name,'passed',flush=True)
    inputs=json.loads((E/(arm+'-source-inputs.json')).read_text());assert all(sha(s/n)==h for n,h in inputs.items())
    (out/'complete.exit').write_text('0\n')
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:list(pool.map(build,['base','gc']))
(E/'complete.exit').write_text('0\n')
