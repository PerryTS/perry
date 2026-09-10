from pathlib import Path
import hashlib,json,os,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];source=w/'memory-worker.ts'
source.write_text((w/'harness/worker.ts').read_text()+'''\n// Diagnostic only: explicit collection follows the measured work.\ndeclare function gc(): void;\nconsole.log('MEMORY_LIVE', process.memoryUsage().rss);\ngc();\nconsole.log('MEMORY_COLLECTED_LIVE', process.memoryUsage().rss);\nif (process.argv[6] === 'verify') console.log('POST_GC_VERIFY', JSON.stringify(last));\nlast = null;\ninput = null;\nretained = [];\ngc();\nconsole.log('MEMORY_COLLECTED_DROPPED', process.memoryUsage().rss);\n''')
build=w/'frozen-main';obj=w/'memory-worker.o';binary=w/'memory-worker';clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
commands=[[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','-o',str(obj)],['cc',str(obj),str(build/'libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)]]
for i,cmd in enumerate(commands):
 with (w/('memory-worker-build-'+str(i)+'.log')).open('wb') as log:subprocess.run(cmd,cwd=root,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
(w/'memory-worker-provenance.json').write_text(json.dumps({'source_commit':'1a9c0de6cb790d2467b0ca22a660870025179b37','diagnostic_only':True,'commands':commands,'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,obj,binary,build/'perry',build/'libperry_runtime.a',build/'libperry_stdlib.a']}},indent=2)+'\n')
print('Compiled frozen-main diagnostic: live result remains observable after GC; explicit collections follow timing.')
