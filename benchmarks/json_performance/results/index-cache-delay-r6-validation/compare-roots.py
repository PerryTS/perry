from pathlib import Path
import hashlib,json,re,shutil
w=Path(__file__).resolve().parent;r5=w.parent/'invariant-field-loop-r5'
current=(w/'ir-native/check-0.log').read_text()
reference=json.loads((r5/'root-fingerprint-comparison.json').read_text())
results=[]
for row in reference:
 fixture=row['fixture']; actual=sorted(re.findall(r'fingerprint\s+: '+fixture+r'-0\.ll::([^\n]+)',current))
 source=Path(__file__).resolve().parents[4]/row['baseline_log']; dest=w/(fixture+'-actual-main-roots.log');shutil.copy2(source,dest)
 baseline=sorted(re.findall(r'fingerprint\s+: [^:]+::([^\n]+)',dest.read_text()))
 results.append({'fixture':fixture,'actual':actual,'actual_main_53df':baseline,'same':actual==baseline,'baseline_log':dest.name,'baseline_log_sha256':hashlib.sha256(dest.read_bytes()).hexdigest()})
assert sum(len(row['actual']) for row in results)==4 and all(row['same'] for row in results),results
(w/'root-fingerprint-comparison.json').write_text(json.dumps(results,indent=2)+'\n')
print('All four unsuppressed fingerprints match the archived actual-main proof on identical fixtures.')
