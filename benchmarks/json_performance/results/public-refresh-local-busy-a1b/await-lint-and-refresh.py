from pathlib import Path
import os,re,subprocess,time
root=Path('/Users/amlug/projects/perry/json-merged-pr10022')
log=Path('/tmp/json-pr10036-current-lint.log')
print('Waiting for the full local lint/compile chain to finish before the canonical refresh.',flush=True)
deadline=time.monotonic()+1800
while True:
 text=log.read_text()
 # The driver writes its final summary only after all compile commands and
 # API-doc generation. Accept exactly the already-proven stale-artifact failure.
 match=re.search(r'^run_lint_gates: (\d+) of (\d+) FAILED[^\n]*\n(.*)\Z',text,re.M|re.S)
 if match:
  assert match.group(1)=='1',text[-4000:]
  failures=[x.strip() for x in match.group(3).splitlines() if x.strip()]
  assert failures==['[Public benchmark evidence freshness] python3 benchmarks/ci_public_baseline_check.py'],failures
  break
 assert time.monotonic()<deadline,'local lint chain did not finish in 30 minutes'
 time.sleep(2)
print('Local gates finished with only the inherited stale-artifact failure. Starting refresh.',flush=True)
raise SystemExit(subprocess.run(['python3','benchmarks/json_performance/.work/run-public-baseline-current.py'],cwd=root).returncode)
