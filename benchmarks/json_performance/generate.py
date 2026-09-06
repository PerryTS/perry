#!/usr/bin/env python3
"""Deterministic fixtures. No fixture construction is in benchmark timing."""
import hashlib, json
from pathlib import Path
ROOT = Path(__file__).resolve().parent
OUT = ROOT / '.work' / 'fixtures'
OUT.mkdir(parents=True, exist_ok=True)
manifest = []
def emit(name, value, kind, ops=None):
    blob = json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()
    (OUT / (name + '.json')).write_bytes(blob)
    manifest.append(dict(name=name, bytes=len(blob), utf16_units=len(blob.decode().encode('utf-16-le'))//2,
                         kind=kind, sha256=hashlib.sha256(blob).hexdigest(),
                         operations=ops or ['parse','stringify']))
def row(i):
    return dict(id=i, name='user_'+str(i), email='user_'+str(i)+'@example.com',
                active=i%3!=0, score=i*1.5, tags=['tag_'+str(i%10),'tag_'+str(i%7)])
for name,value in [('null',None),('string_a','a'),('empty_object',{}),('tiny_object',{'a':1}),('small_record',row(42))]:
    emit(name,value,'small')
emit('object_1k',{'id':1,'message':'x'*1000},'object')
for label,count in [('16k',120),('1m',7600),('8m',59000),('20m',145000)]:
    rows=[row(i) for i in range(count)]
    emit('records_array_'+label,rows,'records_array',['parse','stringify','sparse','scan','roundtrip'])
    if label != '16k': emit('records_object_'+label,{'records':rows},'records_object')
emit('numbers_1m',[i*0.125 for i in range(125000)],'numbers')
emit('long_string_1m',{'id':1,'text':'abcdefgh'*131072},'string')
emit('escaped_1m',{'id':1,'text':'line\n"quote"\\tab\t'*45000},'string')
emit('unicode_1m',{'id':1,'text':'Grüße東京🙂'*52000},'string')
emit('wide_1m',{'field_'+str(i):i for i in range(50000)},'wide')
emit('heterogeneous_1m',[{'id':i,('field_'+str(i%32)):i,'value':str(i),'nested':{'yes':True}} for i in range(14000)],'heterogeneous')
(ROOT/'results'/'fixtures.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Generated',len(manifest),'fixtures:',sum(x['bytes'] for x in manifest),'bytes')
