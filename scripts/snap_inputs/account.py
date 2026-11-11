#!/usr/bin/env python3
"""Join runtime and native counters, reconstruct the census's exact Ir charges."""
import bisect,collections,csv,json,struct,sys
from pathlib import Path

def load(report,native):
 events=list(csv.DictReader(open(str(report)+".inputs.tsv"),delimiter="\t"))
 for e in events:
  e["seq"]=int(e["seq"]);e["ir"]=int(e["ir"]);e["init"]=int(e["init"])
  e["value"]=bytes.fromhex(e["value_hex"]).decode("utf-8")
  e["stack"]=[tuple(map(int,p.split(":"))) for p in e["stack_parts"].split(",") if p]
 raw=[json.loads(x) for x in open(native)]
 opens=[x for x in raw if x.get("nr")==298 and x.get("ret",-1)>=0]
 fdset={x["ret"] for x in opens}
 reads=[x for x in raw if x.get("nr")==0 and x["args"][0] in fdset and x.get("ret")==8 and isinstance(x.get("bytes"),str)]
 groups=collections.defaultdict(list)
 for r in reads:
  r["runtime_ir"]=struct.unpack("<Q",bytes.fromhex(r["bytes"]))[0]
  groups[(r["tid"],r["args"][0])].append(r)
 possible=[]
 for key,rs in groups.items():
  offsets={r["ir_exit"]-r["runtime_ir"] for r in rs}
  if len(offsets)==1:
   observed={r["runtime_ir"] for r in rs}
   if all(e["ir"] in observed for e in events):
    possible.append((key,rs,next(iter(offsets))))
 assert len(possible)==1,[(k,len(rs),offset) for k,rs,offset in possible]
 (tid,fd),reads,offset=possible[0]
 reads.sort(key=lambda x:x["runtime_ir"]);ticks=[r["runtime_ir"] for r in reads]
 markers={int(r["runtime_seq"]):r for r in raw if r["kind"]=="runtime_marker" and r["tid"]==tid}
 charges=[]
 for e in events:
  if e["op"] not in ("init_enter","init_exit","job_enter","job_exit","entry_body","boundary"):continue
  if e["op"] in ("job_exit","boundary"):
   end=markers[e["seq"]]["ir_exit"]-offset
   at=bisect.bisect_right(ticks,end)
  else:
   at=bisect.bisect_left(ticks,e["ir"])-1
  assert 0<=at<len(reads),(e["op"],e["seq"],at)
  stamp=ticks[at]
  stack=e["stack"][:-1] if e["op"] in ("init_enter","job_enter") else e["stack"]
  owner=stack[-1][0] if stack else 0
  charges.append((stamp,owner,e["op"],e["seq"]))
 charges.sort()
 initial=ticks[0]
 sums=collections.Counter();previous=initial
 for stamp,owner,op,seq in charges:
  assert stamp>=previous,(stamp,previous,op)
  sums[owner]+=stamp-previous;previous=stamp
 actual={}
 for line in open(report):
  if line.startswith("#"):continue
  row=line.rstrip("\n").split("\t")
  actual[int(row[0])]=int(row[12])
 mismatches={i:(actual.get(i,0),sums.get(i,0)) for i in set(actual)|set(sums) if actual.get(i,0)!=sums.get(i,0)}
 return events,raw,{"tid":tid,"fd":fd,"offset":offset,"initial":initial,"charges":charges,
   "actual":actual,"reconstructed":dict(sums),"mismatches":mismatches,
   "real_ids":sorted({e["init"] for e in events if e["op"]=="init_enter"})}

def main():
 report=Path(sys.argv[1]);native=Path(sys.argv[2]);out=Path(sys.argv[3])
 events,raw,account=load(report,native)
 out.write_text(json.dumps(account,indent=2)+"\n")
 print("counter_offset",account["offset"],"charges",len(account["charges"]),
   "exact_census_accounting",not account["mismatches"])
 if account["mismatches"]:print("mismatches",account["mismatches"])
 else:
  ids=set(account["real_ids"])
  total=sum(v for i,v in account["actual"].items() if i in ids)
  print("real_init_Ir",total,"real_inits",len(ids))
 return bool(account["mismatches"])
if __name__=="__main__":sys.exit(main())

