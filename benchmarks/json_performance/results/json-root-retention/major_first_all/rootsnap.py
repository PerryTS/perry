import lldb,json,os,struct,hashlib
from pathlib import Path
root=Path(__file__).resolve().parent
state={}; events=[]; errors=[]; mark_bps=[]; reg_by_bp={}
def reg(f,name):
 v=f.FindRegister(name)
 return v.GetData().GetUnsignedInt64(lldb.SBError(),0) if name.startswith('d') else v.GetValueAsUnsigned()
def mem(p,addr,size):
 e=lldb.SBError();b=p.ReadMemory(addr,size,e)
 return b if e.Success() else b''
def frames(f):
 t=f.GetThread();out=[]
 for i in range(min(t.GetNumFrames(),18)):
  x=t.GetFrameAtIndex(i);out.append({'name':x.GetFunctionName() or '', 'sp':x.GetSP(),'fp':x.GetFP(),'pc':x.GetPC(),'file_pc':x.GetPCAddress().GetFileAddress()})
 return out
def object_info(p,addr):
 b=mem(p,addr-8,8)
 if len(b)!=8:return {'address':addr,'read_error':True}
 typ,flags,reserved,size=struct.unpack('<BBHI',b);body=mem(p,addr,min(max(size-8,0),96))
 v={'address':addr,'type':typ,'flags':flags,'reserved':reserved,'size':size,'body_hex':body.hex()}
 if typ==3 and len(body)>=20:
  v['units'],v['bytes'],v['capacity'],v['refcount'],v['string_flags']=struct.unpack('<IIIII',body[:20]);v['prefix']=body[20:].decode('utf8','replace')
 elif typ==1 and len(body)>=8:v['length'],v['capacity']=struct.unpack('<II',body[:8])
 elif typ==2 and len(body)>=16:v['class'],v['shape'],v['meta']=struct.unpack('<IIQ',body[:16])
 return v
def guarded(fn):
 def cb(f,b,extra):
  try:return fn(f,b,extra)
  except BaseException as e:errors.append(repr(e));return True
 return cb
@guarded
def full(f,b,extra):
 state['full']+=1;events.append({'event':'full_begin','full':state['full'],'parse':state['parse'],'frames':frames(f)})
 return False
@guarded
def minor(f,b,extra):
 events.append({'event':'minor_begin','full':state['full'],'parse':state['parse'],'frames':frames(f)})
 return False
@guarded
def major_branch(f,b,extra):
 stack=frames(f)
 assert any('gc_safepoint_moving_minor' in x['name'] for x in stack),stack
 assert not any('perry_fn_lifetime_worker_ts__one' in x['name'] for x in stack),stack
 state['forced_major']+=1
 events.append({'event':'force_major','parse':state['parse'],'frames':stack})
 target=f.GetSymbol().GetStartAddress().GetLoadAddress(f.GetThread().GetProcess().GetTarget())+state['major_target']
 assert f.SetPC(target)
 return False
@guarded
def parse(f,b,extra):
 state['parse']+=1;events.append({'event':'parse_begin','parse':state['parse'],'input':reg(f,'x0')})
 return False
@guarded
def stringify(f,b,extra):
 bits=reg(f,'d0');events.append({'event':'stringify_begin','parse':state['parse'],'input_bits':bits,'input':object_info(f.GetThread().GetProcess(),bits&0xffffffffffff) if bits>>48==0x7ffd else None})
 if state['parse']==27 and not state['output_checked'] and any('perry_fn_lifetime_worker_ts__one' in x['name'] for x in frames(f)):
  bp=f.GetThread().GetProcess().GetTarget().BreakpointCreateByAddress(reg(f,'x30'));bp.SetOneShot(True);bp.SetScriptCallbackFunction('rootsnap.output')
 return False
@guarded
def output(f,b,extra):
 p=f.GetThread().GetProcess();bits=reg(f,'x0');ptr=bits&0xffffffffffff;obj=object_info(p,ptr);assert obj['type']==3 and obj['bytes']>10000000,(bits,obj)
 data=mem(p,ptr+20,obj['bytes']);assert len(data)==obj['bytes']
 sha=hashlib.sha256(data).hexdigest();state['output_checked']=True;state['output_sha256']=sha;events.append({'event':'checked_output','sha256':sha,'bytes':len(data),'matches_node':sha==state['expected_sha256']})
 assert sha==state['expected_sha256'],sha
 return False
@guarded
def begin(f,b,extra):
 state['scan']+=1;events.append({'event':'scan_begin','scan':state['scan'],'full':state['full'],'parse':state['parse'],'frames':frames(f)})
 for bp in mark_bps:bp.SetEnabled(True)
 return False
@guarded
def end(f,b,extra):
 for bp in mark_bps:bp.SetEnabled(False)
 events.append({'event':'scan_end','scan':state['scan'],'full':state['full'],'parse':state['parse']})
 return False
@guarded
def mark(f,b,extra):
 p=f.GetThread().GetProcess();ptr=reg(f,reg_by_bp[b.GetBreakpoint().GetID()]);stack=frames(f);source={}
 for i,x in enumerate(stack):
  offset=x['file_pc']-state['scan_address']-4
  if offset not in state['calls']:continue
  caller=f.GetThread().GetFrameAtIndex(i);buf=state['calls'][offset]
  addr=caller.GetSP()+buf if buf is not None else reg(caller,'x27')-8
  word=mem(p,addr,8)
  source={'scan_call_offset':offset,'buffer_offset':buf,'slot':addr,'word':int.from_bytes(word,'little') if len(word)==8 else None}
  if buf is None:
   for a,c in zip(stack,stack[1:]):
    if a['sp']<=addr<c['sp']:source['owner_frame']=a['name'];source['owner_frame_offset']=addr-a['sp'];break
  break
 skip=bool(state.get('skip_below')) and any(state['skip_below'] in x['name'] and source.get('slot',1<<64)<x['sp'] for x in stack)
 if skip:
  address=f.GetSymbol().GetStartAddress().GetLoadAddress(p.GetTarget())+0x194
  assert f.SetPC(address)
  state['skipped']+=1
 events.append({'event':'root','skipped':skip,'scan':state['scan'],'full':state['full'],'parse':state['parse'],'object':object_info(p,ptr),'source':source,'frames':stack})
 return False
def run(debugger,arm):
 global mark_bps,reg_by_bp
 conf=json.loads((root/'config.json').read_text())[arm]
 worker=Path(conf['worker'])
 assert hashlib.sha256(worker.read_bytes()).hexdigest()==conf['sha256']
 target=debugger.CreateTarget(str(worker));debugger.SetAsync(False);state['expected_sha256']=conf['expected_output_sha256']
 state.update(forced_major=0,major_target=conf['force_major_target'],full=0,parse=0,scan=0,skip_below=conf.get('skip_below'),skipped=0,output_checked=False,calls={x['offset']:x['buffer_offset'] for x in conf['calls']})
 def point(symbol,offset,callback):
  contexts=target.FindSymbols(symbol,lldb.eSymbolTypeCode)
  if contexts.GetSize()==0:contexts=target.FindSymbols(symbol[1:],lldb.eSymbolTypeCode)
  assert contexts.GetSize()>0,symbol
  start=contexts.GetContextAtIndex(0).GetSymbol().GetStartAddress().GetFileAddress()
  bp=target.BreakpointCreateBySBAddress(target.ResolveFileAddress(start+offset));bp.SetScriptCallbackFunction('rootsnap.'+callback);assert bp.GetNumLocations()==1,(symbol,offset)
  return bp,start
 point(conf['full_symbol'],0,'full');point('__RNvNtCs5gMwpk3Cs4e_13perry_runtime2gc35gc_collect_minor_with_trigger_inner',0,'minor');point('_js_json_parse',0,'parse');point('_js_json_stringify_full',0,'stringify')
 point('__RNvNtCs5gMwpk3Cs4e_13perry_runtime2gc35gc_collect_minor_with_trigger_inner',conf['force_major_branch'],'major_branch')
 bp,start=point(conf['scan_symbol'],conf['scan_begin'],'begin');state['scan_address']=start
 point(conf['scan_symbol'],conf['scan_end'],'end')
 for row in conf['points']:
  bp,mark_start=point(conf['symbol'],row['offset'],'mark');state['reject_pc']=target.ResolveFileAddress(mark_start+0x194).GetLoadAddress(target);bp.SetEnabled(False);mark_bps.append(bp);reg_by_bp[bp.GetID()]=row['register']
 args=[str(root/'records_object_20m.json'),'discard','24','roundtrip']
 launch=lldb.SBLaunchInfo(args);launch.SetWorkingDirectory(str(root));launch.SetLaunchFlags(0)
 launch.SetEnvironmentEntries([k+'='+v for k,v in os.environ.items() if not k.startswith('PERRY_')]+['PERRY_GC_TRACE=1'],False)
 launch.AddOpenFileAction(1,str(root/'results'/(arm+'.stdout')),False,True);launch.AddOpenFileAction(2,str(root/'results'/(arm+'.stderr')),False,True)
 error=lldb.SBError();process=target.Launch(launch,error)
 summary={'arm':arm,'worker_sha256':conf['sha256'],'launch_error':str(error),'state':process.GetState(),'exit':process.GetExitStatus(),'errors':errors,'counts':{k:state[k] for k in ['full','parse','scan']},'forced_major':state['forced_major'],'events':len(events),'skipped':state['skipped'],'output_checked':state['output_checked'],'output_sha256':state.get('output_sha256')}
 (root/'results'/(arm+'-roots.jsonl')).write_text(''.join(json.dumps(e)+'\n' for e in events));(root/'results'/(arm+'-summary.json')).write_text(json.dumps(summary,indent=2)+'\n');print('ROOT_SNAPSHOT',json.dumps(summary),flush=True)
 if process.GetState()!=lldb.eStateExited:process.Kill()
 assert error.Success() and process.GetState()==lldb.eStateExited and process.GetExitStatus()==0 and not errors and state['output_checked'],summary
