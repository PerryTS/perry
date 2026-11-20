#!/usr/bin/env python3
"""Facts ledger identities from source-span site records, never count verdict rows."""
import collections,json,os,sys
from pathlib import Path
def canon(p):
 return os.path.realpath(p) if p.startswith("/") else p
def ledger(path):
 sites=collections.defaultdict(set); verdicts=collections.defaultdict(list)
 for line in open(path,errors="strict"):
  if not line.startswith("D {"):continue
  d=json.loads(line[2:])
  if d.get("record")=="site" and "key" in d and d.get("kind") in ("property","element","value-call","new"):
   call=d["kind"] in ("value-call","new")
   key=(d.get("unit"),canon(d["file"]),d["start"],d["key"],call)
   sites[key].add((canon(d["file"]),d["start"],d["end"],d["kind"],d["key"]))
  elif d.get("record")=="verdict":
   key=(d.get("unit"),canon(d["file"]),d["start"],d["key"],d["call"])
   verdicts[key].append(not d["verdict"].startswith("No"))
 proved=set();missing=[];mixed=[]
 for key,vs in verdicts.items():
  if any(vs) and not all(vs):mixed.append(key)
  if all(vs):
   ids=sites.get(key)
   if not ids:missing.append(key)
   else:proved.update(ids)
 return proved,{"verdict_rows":sum(map(len,verdicts.values())),"groups":len(verdicts),
  "mixed":mixed,"unmatched_proven":missing}
def main():
 root=Path(sys.argv[1]); out=Path(sys.argv[2]);out.mkdir(exist_ok=True)
 names=["hello","commander_help","commander","fastify_inject","awfy_richards","binary_trees"]
 summary=[]
 for name in names:
  a,da=ledger(root/f"{name}.main.facts.dump");b,db=ledger(root/f"{name}.head.facts.dump")
  lost=sorted(a-b);gained=sorted(b-a)
  (out/f"{name}.main.json").write_text(json.dumps(sorted(a),indent=2)+"\n")
  (out/f"{name}.head.json").write_text(json.dumps(sorted(b),indent=2)+"\n")
  (out/f"{name}.lost.json").write_text(json.dumps(lost,indent=2)+"\n")
  (out/f"{name}.gained.json").write_text(json.dumps(gained,indent=2)+"\n")
  (out/f"{name}.diagnostics.json").write_text(json.dumps({"main":da,"head":db},indent=2)+"\n")
  complete=not(da["unmatched_proven"] or db["unmatched_proven"] or da["mixed"] or db["mixed"])
  summary.append((name,len(a),len(b),len(lost),len(gained),complete))
  print(*summary[-1],sep="\t")
  if not complete:print("INCOMPLETE",name,da,db)
 (out/"summary.tsv").write_text("witness\tmain\thead\tlost\tgained\tcomplete\n"+"".join("\t".join(map(str,r))+"\n" for r in summary))
 return int(any(not r[-1] or r[3] for r in summary))
if __name__=="__main__":sys.exit(main())

