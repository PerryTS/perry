from pathlib import Path
import subprocess
root=Path.cwd()
for name,op in [('numbers_1m','parse'),('records_array_8m','parse'),('heterogeneous_1m','parse'),('escaped_1m','stringify'),('small_record','stringify'),('long_string_1m','stringify')]:
 out=root/'results/recheck-direct'/(name+'-'+op)
 assert not out.exists()
 subprocess.run(['python3',str(root/'run.py'),'--phase','timing','--repeat','7','--filter',name,'--operations',op,'--worker',str(root/'.work/worker'),'--baseline-worker',str(root/'.work/baseline-worker'),'--results-dir',str(out)],check=True)
