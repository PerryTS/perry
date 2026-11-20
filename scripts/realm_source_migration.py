#!/usr/bin/env python3
"""One-time, line-preserving migration record; never used by a build.

The baseline manifest is an oracle for the migration, not an input of the new
producer. Only human facts are emitted. Unsupported births are reported rather
than embedded as reflected records.
"""
from pathlib import Path
import argparse
import collections
import json
import re
import struct

def tokens(s):
    i=0
    while i<len(s):
        if s[i].isspace(): i+=1;continue
        if s.startswith('//',i):
            end=s.find('\n',i);i=len(s) if end<0 else end;continue
        if s.startswith('/*',i):
            depth=1;i+=2
            while depth:
                if s.startswith('/*',i):depth+=1;i+=2
                elif s.startswith('*/',i):depth-=1;i+=2
                else:i+=1
            continue
        raw=re.match(r'(?:[bc])?r(#{0,255})"',s[i:])
        if raw:
            end=s.index('"'+raw[1],i+raw.end())+1+len(raw[1])
        else:
            m=re.match(r'(?:[bc])?"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])\'|[A-Za-z_][A-Za-z_0-9]*|[0-9][A-Za-z_0-9.]*|::|<<|>>|=>|.',s[i:],re.S)
            end=i+m.end()
        yield s[i:end],i,end
        i=end
def split(ts,sep=','):
    out=[];start=0;depth=0
    for i,t in enumerate(ts):
        if t[0] in ('(','[','{'):depth+=1
        if t[0] in (')',']','}'):depth-=1
        if t[0]==sep and depth==0:out.append(ts[start:i]);start=i+1
    if ts[start:]:out.append(ts[start:])
    return out
def joined(ts):return ''.join(t[0] for t in ts)
def number(expr):
    expr=re.sub(r'(?:[A-Za-z_]\w*::)+','',expr).replace(' as u32','')
    if re.fullmatch(r'[0-9xa-fA-F_]+',expr):return int(expr.replace('_',''),0) if expr.startswith('0x') else int(expr.replace('_',''))
    return eval(expr,{'__builtins__':{}},CONSTANTS)
def signature(ts):
    args=split(ts,';')
    lead=split(args[0])
    b=lead[0][:]
    native=b[0][0]=='native_args'
    if b[0][0] in ('native_args','unwind','unwind_in_tests'): b=b[1:]
    body=joined(b)
    n=number(joined(lead[1]))
    f=dict(params=0 if native else n,rest_fixed=n if native else 0,flags=CONSTANTS['FN_REST_NATIVE_ARGS'] if native else 0,length=0,declared=0,plain_params=0)
    for call in split(args[1]) if len(args)>1 else []:
        name=call[0][0]
        values=[number(joined(x)) for x in split(call[2:-1])]
        if name=='with_flags':f['flags']|=values[0]
        elif name=='with_declared':f['declared']=values[0];f['flags']|=CONSTANTS['FN_HAS_DECLARED']
        elif name=='with_length':f['length']=values[0];f['flags']|=CONSTANTS['FN_HAS_LENGTH']
        elif name in ('with_rest','with_rest_kind'):
            f['rest_fixed']=values[0];f['flags']=(f['flags']&~CONSTANTS['FN_REST_MASK'])|(CONSTANTS['FN_REST_USER'] if name=='with_rest' else values[1])
        elif name=='plain':f['plain_params']=f['params']+1
        else:raise ValueError(name)
        if f['flags']&CONSTANTS['FN_REST_MASK']:f['plain_params']=0
    return body,f

CONSTANTS={}
FAMILIES={}
def abi_constants(root):
    abi=(root/'crates/perry-abi/src/lib.rs').read_text()
    values={}
    for name,expr in re.findall(r'pub const ((?:FN_|CLOSURE_|GC_LAYOUT_|TAG_)\w+): u(?:16|32|64) =\s*([^;]+);',abi):
        try:
            expr=expr.replace('_','') if re.fullmatch(r'[0-9xa-fA-F_ <>|]+',expr) else expr
            values[name]=eval(expr,{'__builtins__':{}},values)
        except (NameError,SyntaxError,TypeError): pass
    return values


def constructor_families(root):
    source=root/'crates/perry-runtime/src/object/global_this_tables.rs'
    if not source.exists():return {}
    source_text=source.read_text()
    ts=list(tokens(source_text))
    start=next(i+3 for i in range(len(ts)-2)if joined(ts[i:i+3])=='builtin_constructors!{')
    end=matching(ts,start-1)
    families={}
    rows=split(ts[start:end])
    for index,row in enumerate(rows):
        clauses=split(row,';')
        name=clauses[0][0][0]
        fields=dict(identity=index+1,prototype_callable=False)
        for clause in clauses[1:]:
            key=clause[0][0]
            output=dict(class_='prototype_class',serial='prototype_serial',sealed='sealed_serial',named_serial='named_serial',instance='instance_identity',prototype_callable='prototype_callable').get('class_'if key=='class'else key)
            if output is None:raise ValueError(('unknown constructor fact',key))
            value=joined(clause[1:]).replace('crate::typedarray_hook_slots::','crate::typedarray::')
            if key=='instance' and clause[-1][2]==row[-1][2]:
                next_start=rows[index+1][0][1] if index+1<len(rows)else ts[end][1]
                tail=source_text[row[-1][2]:next_start].rstrip()
                if '//'in tail:value+=tail.split(';')[0]
            fields[output]=(value=='true')if key=='prototype_callable'else value
        families[name]=fields
    return families


def matching(ts,start):
    pairs={'(':')','[':']','{':'}'}
    stack=[]
    for i in range(start,len(ts)):
        token=ts[i][0]
        if token in pairs:stack.append(pairs[token])
        elif stack and token==stack[-1]:
            stack.pop()
            if not stack:return i
    raise ValueError('unterminated source token group')

def cfg_without_tests(ts):
    """Three-valued cfg evaluation: test=false, unrelated switches unknown."""
    if joined(ts)=='test':return False
    if len(ts)>=3 and ts[0][0]in ('all','any','not') and ts[1][0]=='(' and ts[-1][0]==')':
        values=[cfg_without_tests(arg)for arg in split(ts[2:-1])]
        if ts[0][0]=='not':return None if len(values)!=1 or values[0]is None else not values[0]
        if ts[0][0]=='all':return False if False in values else True if all(v is True for v in values)else None
        return True if True in values else False if all(v is False for v in values)else None
    return None

def test_ranges(ts):
    """Skip only cfg(test)-disabled items, not the remainder of their files."""
    ranges=[];i=0
    while i+3<len(ts):
        if [t[0]for t in ts[i:i+4]]!=['#','[','cfg','(']:
            i+=1;continue
        attr_end=matching(ts,i+1)
        if cfg_without_tests(ts[i+4:attr_end-1])is not False:
            i=attr_end+1;continue
        j=attr_end+1
        look=j
        while look<len(ts) and ts[look][0]in ('pub','unsafe','async','extern'):
            look+=1
            if look<len(ts) and ts[look][0]=='(':look=matching(ts,look)+1
            if look<len(ts) and ts[look][0].startswith('"'):look+=1
        item=look<len(ts) and ts[look][0]in ('fn','struct','enum','impl','mod','const','static','let','macro_rules','type','trait','union','use')
        angles=0
        while j<len(ts):
            token=ts[j][0]
            if token=='<':angles+=1
            if token=='>' and angles:angles-=1
            if token==',' and not angles and not item:j+=1;break
            if token in ('(','['):j=matching(ts,j)+1;continue
            if token=='{':
                j=matching(ts,j)+1;break
            if token==';':j+=1;break
            j+=1
        ranges.append((ts[i][1],ts[j-1][2]))
        i=j
    return ranges


def scan(root):
    sites=collections.defaultdict(list)
    for path in sorted((root/'crates').glob('*/src/**/*.rs')):
        if 'tests' in path.parts or path.stem.endswith('_tests'):continue
        s=path.read_text()
        if 'fn_info!' not in s and 'js_function_info!' not in s:continue
        ts=list(tokens(s))
        excluded=test_ranges(ts)
        for i,t in enumerate(ts):
            if t[0]not in ('fn_info','js_function_info') or [x[0] for x in ts[i+1:i+3]]!=['!','(']:continue
            depth=1;j=i+3
            while depth:
                if ts[j][0]=='(':depth+=1
                if ts[j][0]==')':depth-=1
                j+=1
            body_tokens=ts[i+3:j-1]
            if any(t[0]=='$' for t in body_tokens) or not body_tokens or body_tokens[0][0]=='@':continue
            annotation=None
            if body_tokens[0][0]=='#':
                end=matching(body_tokens,1)
                attribute=body_tokens[:end+1]
                if joined(attribute[:4])!='#[realm(':continue
                inner=attribute[4:-2]
                if joined(inner[:3])=='realm_decl!{':
                    previous=[s[part[0][1]:part[-1][2]]for part in split(inner[3:-1],';')]
                else:previous=[s[inner[0][1]:inner[-1][2]]]
                annotation=dict(start=attribute[0][1],end=attribute[-1][2],facts=previous)
                body_tokens=body_tokens[end+1:]
            # A local metadata binding is intentionally one physical cell;
            # named build projections can coexist in a shared-body block.
            if len(split(body_tokens,';'))>2:continue
            try:body,f=signature(body_tokens)
            except (ValueError,NameError,IndexError,SyntaxError):continue
            if any(start<=t[1]<end for start,end in excluded):continue
            sites[body.split('::')[-1]].append(dict(
                path=str(path.relative_to(root)),line=s[:t[1]].count('\n')+1,
                offset=ts[i+2][2],body=body,signature=f,annotation=annotation))
    return sites

def visible_name(name):
    prefix=''
    for role in ('get','set'):
        if name.endswith('.<'+role+'>'):
            name=name[:-6];prefix=role+' ';break
    if name.endswith(']'):
        leaf=json.loads(name[name.rfind('[')+1:-1])
        if leaf.startswith('@@'):leaf='[Symbol.'+leaf[2:]+']'
    else:leaf=name.rsplit('.',1)[-1]
    return prefix+leaf

def default_kind(name):
    if name.endswith('.<get>'):return 'getter'
    if name.endswith('.<set>'):return 'setter'
    return 'method' if '.' in name or '[' in name else 'function'

def default_length(f):
    return f['length'] if f['flags']&8 else f['declared'] if f['flags']&2048 else f['rest_fixed'] if f['flags']&65543 else f['params']

def bare_length(info):
    if info['flags'] & CONSTANTS['FN_HAS_LENGTH']:return info['length']
    if info['flags'] & CONSTANTS['FN_REST_MASK']:return info['rest_fixed']
    if info['flags'] & CONSTANTS['FN_HAS_DECLARED']:return info['declared']
    return 0

def simple_facts(d):
    """Return human facts only if the producer's defaults reproduce all bytes."""
    f=d.get('function_info')
    if not f or f['native_method']:return None
    kind=d['kind']
    if kind not in ('method','function','getter','setter','constructor'):return None
    length=default_length(f)
    key=visible_name(d['name'])
    if kind=='getter':key=key.removeprefix('get ')
    if kind=='setter':key=key.removeprefix('set ')
    key_captures=[dict(kind='bytes',value=key),dict(kind='raw_word',value=hex(len(key.encode())))]
    capture_key=d.get('captures')==key_captures
    bare_closure=('TAG_UNDEFINED'in CONSTANTS and d.get('captures')==[dict(kind='raw_word',value=hex(CONSTANTS['TAG_UNDEFINED']))])
    empty_closure=(d.get('captures')==[] and any(p['key']=='name'and p['value']==dict(kind='string',value='')for p in d.get('properties',[])))
    bare_closure=bare_closure or empty_closure
    allocated_closure=empty_closure and d.get('header',{}).get('reserved')==CONSTANTS.get('GC_LAYOUT_POINTER_FREE')
    if bare_closure:length=bare_length(f)
    constructible=(not bare_closure and kind!='constructor' and d.get('own_key_order')==['length','name','prototype'])
    if constructible and f['flags']&512:return None
    expected={
        'name':d['name'],'kind':kind,
        'properties':[
            dict(key='length',symbol=False,attributes=4,value=dict(kind='number_bits',value=struct.pack('>d',length).hex())),
            dict(key='name',symbol=False,attributes=4,value=dict(kind='string',value='' if bare_closure else visible_name(d['name']))),
        ],
        'own_key_order':['length','name','prototype'] if constructible or (bare_closure and kind!='constructor' and not f['flags']&512) else ['length','name'],'own_symbol_order':[],
        'header':dict(gc_type=4,reserved=CONSTANTS['GC_LAYOUT_POINTER_FREE'] if bare_closure and (allocated_closure or not empty_closure) else 0 if bare_closure or constructible or kind=='constructor' or f['flags']&512 else 128,capture_flags=0),
        'prototype':'Function.prototype','length':length,
        'body':d['body'],'function_info':f,'captures':[] if empty_closure else [dict(kind='raw_word',value=hex(CONSTANTS['TAG_UNDEFINED']))] if bare_closure else key_captures if capture_key else [],
        'info_symbol':d['info_symbol'],'births':d['births'],
    }
    builtin=False
    if kind=='constructor':
        expected['construct']='crate::object::js_new_function_construct'
        expected['properties'].append(dict(key='prototype',symbol=False,attributes=0,
                                           value=dict(kind='reference',value=d['name']+'.prototype')))
        expected['own_key_order'].append('prototype')
        family=FAMILIES.get(d['name'])
        if family is not None:
            expected['constructor']=family
            identity=family['identity']
            expected['captures']=[dict(kind='int32',value=identity)]
            expected['header']['capture_flags']=(identity<<CONSTANTS['CLOSURE_CONSTRUCT_SHIFT'])|CONSTANTS['CLOSURE_NO_THIS_REBIND_FLAG']
            builtin=True
    actual={k:v for k,v in d.items() if k not in ('source','line')}
    if actual!=expected:return None
    facts={}
    if bare_closure:
        facts['provider']='closure_alloc' if allocated_closure else 'closure'
        if not empty_closure:facts['receiver']='undefined'
    if capture_key:facts['capture']='property_key'
    if builtin:facts['provider']='builtin_constructor'
    if constructible:facts['constructible']=True
    if kind!=default_kind(d['name']):facts['kind']=kind
    births=[b for b in d['births'] if b!='Function.prototype' and not(kind=='constructor' and b==d['name']+'.prototype')]
    if births:facts['births']=births
    return facts

def render_path(name):
    return name if re.fullmatch(r'[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*',name) else json.dumps(name,ensure_ascii=False)

def render(name,facts):
    fields=[render_path(facts.get('birth_name',name))]
    if 'alias'in facts:fields.append('alias = '+render_path(facts['alias']))
    if 'kind'in facts:fields.append('kind = '+facts['kind'])
    if 'provider'in facts:fields.append('provider = '+facts['provider'])
    if 'capture'in facts:fields.append('capture = '+facts['capture'])
    if 'receiver'in facts:fields.append('receiver = '+render_path(facts['receiver']))
    if facts.get('constructible'):fields.append('constructible = true')
    for key in ('extends','instance'):
        if key in facts:fields.append(key+' = '+render_path(facts[key]))
    if 'attrs'in facts:fields.append('attrs = '+facts['attrs'])
    if 'members'in facts:fields.append('members = '+facts['members'])
    for field in ('spec','emit'):
        if field in facts:fields.append(field+' = '+facts[field])
    if 'births'in facts:fields.append('births = ['+', '.join(map(render_path,facts['births']))+']')
    return ', '.join(fields)

def insert_at_sites(source,insertions):
    before=source.count('\n')
    for offset,attribute in sorted(insertions,reverse=True):
        source=source[:offset]+attribute+source[offset:]
    assert source.count('\n')==before
    return source

def placement_rank(site, body, old_source):
    reference=site['body'].removeprefix('crate::')
    qualified_match=body.removeprefix('crate::')==reference or body.endswith('::'+reference)
    return (not qualified_match, site['path']!=old_source, site['path'], site['line'])

def migrate(root,baseline,output,apply=False):
    global CONSTANTS,FAMILIES
    CONSTANTS=abi_constants(root)
    FAMILIES=constructor_families(root)
    sites=scan(root)
    baseline={d['name']:d for d in baseline['declarations']}
    placements=[];unplaced=[];groups=collections.defaultdict(list);selected={}
    companions={}
    for file in sorted((root/'crates').glob('*/src/**/*.realm.json')):
        doc=json.loads(file.read_text());companions[file]=doc
        for ds in doc['groups'].values():
            for d in ds:
                oracle=baseline[d['name']]
                assert d=={k:v for k,v in oracle.items() if k not in ('source','line')}
                human=simple_facts(d)
                candidates=sites[d.get('body','').split('::')[-1]]
                expected=d.get('function_info',{})
                candidates=[site for site in candidates if all(expected.get(k)==v for k,v in site['signature'].items())]
                if human is None or not candidates:
                    unplaced.append(dict(name=d['name'],reason='birth requires structured specialization' if human is None else 'no direct fn_info site'))
                    continue
                old='crates/'+oracle['source']
                site=min(candidates,key=lambda site:placement_rank(site,d['body'],old))
                groups[(site['path'],site['offset'])].append((d['name'],human))
                selected[(site['path'],site['offset'])]=site
                placements.append(dict(name=d['name'],old_source=old,old_line=oracle['line'],
                    source=site['path'],line=site['line'],body=d['body'],form='fn_info'))
    byfile=collections.defaultdict(list)
    for (file,offset),entries in groups.items():
        entries.sort()
        annotation=selected[(file,offset)]['annotation']
        rendered=(annotation['facts'] if annotation else [])+[render(*entry)for entry in entries]
        inner=rendered[0] if len(rendered)==1 else 'realm_decl! { '+'; '.join(rendered)+'; }'
        start,end=(annotation['start'],annotation['end']) if annotation else (offset,offset)
        byfile[file].append((start,end,'#[realm('+inner+')]' + ('' if annotation else ' ')))
    moved={d['name'] for d in placements}
    deleted=[]
    if apply:
        for file,insertions in byfile.items():
            path=root/file
            source=path.read_text();before=source.count('\n')
            for start,end,text in sorted(insertions,reverse=True):
                source=source[:start]+text+source[end:]
            assert source.count('\n')==before
            path.write_text(source)
        for file,doc in companions.items():
            for group,ds in doc['groups'].items():
                doc['groups'][group]=[d for d in ds if d['name']not in moved]
            if any(doc['groups'].values()):
                file.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
            else:
                deleted.append(str(file.relative_to(root)))
                file.unlink()
                source=file.with_name(file.name.replace('.realm.json','.rs'))
                text=source.read_text()
                carrier=re.compile(r'^(?:crate::|perry_runtime::)?realm_decl!\("'+re.escape(file.name)+r'#\*"\);[^\n]*$',re.M)
                updated,count=carrier.subn('',text)
                assert count==1,(source,count)
                assert text.count('\n')==updated.count('\n')
                source.write_text(updated.rstrip()+"\n")
    output.mkdir(parents=True,exist_ok=True)
    report=dict(placements=placements,unplaced=unplaced,deleted_companions=deleted,
        annotated_sites=len(groups),entries_moved=len(placements),
        cross_file_entries=sum(p['source']!=p['old_source'] for p in placements),
        cross_file_destinations=dict(collections.Counter(p['source'] for p in placements if p['source']!=p['old_source'])))
    (output/'migration-simple.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items() if k not in ('placements','unplaced','cross_file_destinations','deleted_companions')})
    print('unplaced',len(unplaced),'deleted companions',len(deleted))
    return report


def logical_facts(d):
    if 'header' in d:return None
    facts={};edges=set()
    if 'native_namespace'in d:
        facts['provider']='native'
        module=d['name'].removeprefix('node:') if d['name'].startswith('node:') else d['name'].removeprefix('%NativeNamespace:').removesuffix('%')
        assert module==d['native_namespace']
        expected=dict(name=d['name'],kind='namespace' if d['name'].startswith('node:') else 'exotic',
                      native_namespace=module,exotic_vtable='crate::object::native_module::NATIVE_MODULE_VTABLE_IMPL',
                      own_symbol_order=[],births=d['births'])
        if d['name'].startswith('node:'):
            expected.update(module=module,body='crate::object::native_module::js_create_native_module_namespace')
    elif d['kind']=='constant':
        value=d['value'];kind=value['kind']
        if kind=='undefined':literal='undefined'
        elif kind=='number_bits':
            number=struct.unpack('>d',bytes.fromhex(value['value']))[0]
            literal='NaN' if number!=number else 'Infinity' if number==float('inf') else '-Infinity' if number==float('-inf') else repr(number)
        else:raise ValueError(('unsupported constant',d['name'],value))
        facts['value']=literal
        if d['attributes']!=0:facts['attrs']=d['attributes']
        expected=dict(name=d['name'],kind='constant',value=value,attributes=d['attributes'],births=d['births'],own_symbol_order=[])
    elif d['kind']=='alias':
        expected=dict(name=d['name'],kind='alias',attributes=d['attributes'],births=d['births'],own_symbol_order=[])
        if d['attributes']!=5:facts['attrs']=d['attributes']
        for role in ('get','set'):
            if role in d:facts[role]=d[role];edges.add(d[role]);expected[role]=d[role]
        if not edges:
            if len(d['births'])!=1:raise ValueError(('ambiguous alias',d['name']))
            facts['alias']=d['births'][0];edges.add(d['births'][0])
    else:return None
    assert expected=={k:v for k,v in d.items()if k not in ('source','line')},d['name']
    births=[name for name in d['births']if name not in edges]
    if births:facts['births']=births
    return facts

def render_logical(name,facts):
    parts=[render_path(facts.get('birth_name',name))]
    for key,value in facts.items():
        if key=='birth_name':continue
        if key=='births':value='['+', '.join(map(render_path,value))+']'
        elif key in ('get','set','alias','receiver','extends','instance'):value=render_path(value)
        else:value=str(value)
        parts.append(key+' = '+value)
    return ', '.join(parts)

def migrate_logical(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    targets={
        'provider':('crates/perry-runtime/src/object/native_module.rs','let obj = js_object_alloc(NATIVE_MODULE_CLASS_ID'),
        'global':('crates/perry-runtime/src/object/global_this/populate.rs','fn populate_global_this_builtins_inner('),
    }
    groups=collections.defaultdict(list);companions={};placements=[]
    for file in sorted((root/'crates').glob('*/src/**/*.realm.json')):
        doc=json.loads(file.read_text());companions[file]=doc
        for ds in doc['groups'].values():
            for d in ds:
                human=logical_facts(d)
                if human is None:continue
                key='provider' if 'provider'in human else 'global'
                groups[key].append((d['name'],human))
    for key,entries in groups.items():
        file,needle=targets[key]
        path=root/file;source=path.read_text()
        anchor=source.index(needle)
        blanks=list(re.finditer(r'(?m)^ *$',source[:anchor]))
        if not blanks:raise ValueError(('no line-preserving birth slot',file,needle))
        slot=blanks[-1]
        offset=slot.start()
        line=source[:offset].count('\n')+1
        indent='    ' if key=='provider' else ''
        text=indent+'crate::realm_decl! { '+'; '.join(render_logical(*entry)for entry in sorted(entries))+'; }'
        for name,human in entries:
            old=oracle[name]
            placements.append(dict(name=name,source=file,line=line,old_source='crates/'+old['source'],old_line=old['line'],form='realm_decl'))
        if apply:path.write_text(insert_at_sites(source,[(offset,text)]))
    moved={p['name']for p in placements}
    if apply:
        for file,doc in companions.items():
            for group,ds in doc['groups'].items():doc['groups'][group]=[d for d in ds if d['name']not in moved]
            assert any(doc['groups'].values()),file
            file.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
    report=dict(placements=placements,entries_moved=len(placements),annotated_sites=len(groups),
                cross_file_entries=sum(p['source']!=p['old_source']for p in placements))
    output.mkdir(parents=True,exist_ok=True)
    (output/'migration-logical.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
    return report


def native_default_facts(d):
    f=d.get('function_info')
    if not f or not f['native_method']:return None
    captures=d['captures'];label=visible_name(d['name'])
    if d.get('own_key_order') not in (['length','name'],['length','name','prototype']):return None
    if len(captures)!=3 or captures[0]['kind']!='reference':return None
    expected={
        'name':d['name'],'kind':d['kind'],
        'properties':[
            dict(key='length',symbol=False,attributes=4,value=dict(kind='number_bits',value='0000000000000000')),
            dict(key='name',symbol=False,attributes=4,value=dict(kind='string',value=label)),
        ],
        'own_key_order':d['own_key_order'],'own_symbol_order':[],
        'header':dict(gc_type=4,reserved=0,capture_flags=0),
        'prototype':'Function.prototype','length':0,
        'body':'crate::closure::BOUND_METHOD_FUNC_PTR',
        'function_info':dict(params=0,rest_fixed=0,flags=0,length=0,declared=0,plain_params=0,
                             source_symbol='source:No symbol matches 0xbadddead.',native_method=True),
        'captures':[captures[0],dict(kind='bytes',value=label),dict(kind='raw_word',value=hex(len(label.encode())))],
        'info_symbol':d['info_symbol'],'births':d['births'],
    }
    prototype_attrs=0
    instance=d['name']+'.prototype'
    if d['kind']=='constructor':
        prototype_prop=next((p for p in d['properties']if p['key']=='prototype'),None)
        if not prototype_prop or prototype_prop['value']['kind']!='reference':return None
        prototype_attrs=prototype_prop['attributes']
        instance=prototype_prop['value']['value']
        if prototype_attrs not in range(8):return None
        expected['prototype']=d['prototype']
        expected['properties'].append(dict(key='prototype',symbol=False,attributes=prototype_attrs,value=dict(kind='reference',value=instance)))
        expected['construct']='crate::object::js_new_function_construct'
    if expected!={k:v for k,v in d.items()if k not in ('source','line')}:return None
    facts=dict(provider='bound_method',receiver=captures[0]['value'])
    if prototype_attrs:facts['attrs']='{prototype = '+' | '.join(name for bit,name in [(1,'writable'),(2,'enumerable'),(4,'configurable')]if prototype_attrs&bit)+'}'
    if d['prototype']!='Function.prototype':facts['extends']=d['prototype']
    if d['kind']=='constructor'and instance!=d['name']+'.prototype':facts['instance']=instance
    if d['kind']!='method':facts['kind']=d['kind']
    defaults={d['prototype'],facts['receiver']}
    if d['kind']=='constructor':defaults.add(instance)
    births=[b for b in d['births']if b not in defaults]
    if births:facts['births']=births
    return facts

def migrate_native_defaults(root,baseline,output,apply=False):
    oracle={d['name']:d for d in baseline['declarations']}
    companions={};entries=[]
    for file in sorted((root/'crates').glob('*/src/**/*.realm.json')):
        doc=json.loads(file.read_text());companions[file]=doc
        for ds in doc['groups'].values():
            for d in ds:
                human=native_default_facts(d)
                if human is not None:entries.append((d['name'],human))
    file='crates/perry-runtime/src/closure/registry.rs';path=root/file;source=path.read_text()
    start=source.index('pub static BOUND_METHOD_INFO:')
    end=source.index(';',start)+1
    following=source.index('\n',end)+1
    entries_text=('; '.join(render_logical(*entry)for entry in sorted(entries))+'; ') if entries else ''
    if source.startswith('crate::realm_decl! {',following):
        line_end=source.index('\n',following)
        offset=source.rindex('}',following,line_end)
        text=entries_text
    else:
        blank=re.search(r'(?m)^$',source[following:])
        if blank is None:raise ValueError('no line-preserving native-method birth slot')
        offset=following+blank.start()
        text='crate::realm_decl! { '+entries_text+'}'
    line=source[:offset].count('\n')+1
    placements=[dict(name=name,source=file,line=line,old_source='crates/'+oracle[name]['source'],
                     old_line=oracle[name]['line'],form='realm_decl')for name,_ in entries]
    moved={p['name']for p in placements}
    if apply:
        path.write_text(insert_at_sites(source,[(offset,text)]))
        for file,doc in companions.items():
            for group,ds in doc['groups'].items():doc['groups'][group]=[d for d in ds if d['name']not in moved]
            assert any(doc['groups'].values()),file
            file.write_text(json.dumps(doc,ensure_ascii=False,indent=2)+'\n')
    report=dict(placements=placements,entries_moved=len(placements),annotated_sites=1,
                cross_file_entries=sum(p['source']!=p['old_source']for p in placements))
    output.mkdir(parents=True,exist_ok=True)
    (output/'migration-native-defaults.json').write_text(json.dumps(report,indent=2)+'\n')
    print({k:v for k,v in report.items()if k!='placements'})
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('baseline',type=Path)
    parser.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1])
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--apply',action='store_true')
    parser.add_argument('--logical',action='store_true')
    parser.add_argument('--native-defaults',action='store_true')
    args=parser.parse_args()
    run=migrate_native_defaults if args.native_defaults else migrate_logical if args.logical else migrate
    run(args.root,json.loads(args.baseline.read_text()),args.output,args.apply)

if __name__=='__main__':main()
