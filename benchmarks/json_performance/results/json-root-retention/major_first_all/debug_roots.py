from pathlib import Path
import subprocess,json
root=Path(__file__).resolve().parent
for arm in ['checkpoint','admission']:
 cmd=['xcrun','lldb','--batch','-o','command script import '+str(root/'rootsnap.py'),'-o',"script rootsnap.run(lldb.debugger, '"+arm+"')"]
 with (root/'results'/(arm+'-lldb.log')).open('w') as log:subprocess.run(cmd,check=True,stdout=log,stderr=subprocess.STDOUT,timeout=180)
 summary=json.loads((root/'results'/(arm+'-summary.json')).read_text())
 assert summary['exit']==0 and not summary['errors'] and summary['counts']['parse']==27,summary
 print('DEBUGGED',arm,summary['counts'],flush=True)
