from pathlib import Path
import hashlib,json,shutil
w=Path(__file__).resolve().parent;root=w.parents[3];old=w.parent/'template-call-boundary-r15'
files=['build-workers.py','validate-fixtures.py','validate-options.py','probe-lazy-baseline.py','validate-fraction.py','check-roots.py','compare-roots.py','inspect-machine.py','inspect-cached.py','compare-cached-machine.py','stage.py','archive-results.py','archive-validation.py','measure-rotating.py','analyze-rotating.py','measure-full.py','analyze-full.py','measure.py','analyze-extras.py','measure-screen.py','analyze-screen.py','run-access.py','run-focus.py','run-options.py','run-rotating.py','run-retained.py','measure-retained.py','analyze-retained.py','full-cases.json','screen-cases.json','prior-entry.ts','callback-only.ts','options-worker.ts','options-worker.js','test_json_source_length.ts','lazy-space-probe.ts','lazy-spacer.ts','fraction-spacer.ts','fraction-baseline.json','main-build-provenance.json']
for name in files:
 src=old/name
 if not src.exists():
  print('Not copied (absent optional driver)',name);continue
 shutil.copy2(src,w/name)
for n in ['validate-fixtures.py','check-roots.py']:
 p=w/n;p.write_text(p.read_text().replace('test_json_cached_template_borrow','test_json_template_capture'))
p=w/'stage.py';p.write_text(p.read_text().replace('json-r15-stage-','json-r16-stage-'))
p=w/'compare-cached-machine.py';p.write_text(p.read_text().replace("assert not any(r['symbol'] in ['_memcpy', '_memmove'] for r in records[1]['external_calls'])","assert any(r['symbol'] == '_memcpy' for r in records[1]['external_calls'])"))
p=w/'screen-cases.json';data=json.loads(p.read_text());data['selection']='Predeclared R16 control screen retains R15\'s seven large concerns, three small parse cases and two stringify controls. R16 targets capture on changing small inputs; rotating controls run first. Full qualification scope is unchanged.';p.write_text(json.dumps(data,indent=2)+'\n')
shutil.copytree(old/'harness',w/'harness')
shutil.copy2(root/'test-files/test_json_template_capture.ts',w/'test_json_template_capture.ts')
shutil.copytree(old/'frozen-main',w/'frozen-main')
meta=json.loads((w/'main-build-provenance.json').read_text())
for n,r in meta['files'].items():assert hashlib.sha256((w/'frozen-main'/n).read_bytes()).hexdigest()==r['sha256']
(w/'reference-main.json').write_text(json.dumps({'reused_from':str((old/'frozen-main').relative_to(root)),'source_commit':meta['source_commit'],'files':meta['files'],'note':'Original fresh-main artifacts reused by verified hashes; workers/fixtures/IR recompiled at R16 paths.'},indent=2)+'\n')
for p in w.glob('*.py'):compile(p.read_text(),p.name,'exec')
print('Prepared R16 drivers and verified frozen main hashes; no R16 performance evidence yet.')
