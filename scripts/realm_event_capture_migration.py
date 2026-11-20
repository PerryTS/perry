#!/usr/bin/env python3
"""Bind accessor captures to the existing initial-value row and getter cell."""
import argparse
import copy
import json
from pathlib import Path
import realm_source_migration as m
NAMES={
    'node:events.EventEmitter.defaultMaxListeners.<get>':'[number_property]',
    'node:events.EventEmitter.defaultMaxListeners.<set>':'[ref("node:events.EventEmitter.defaultMaxListeners.<get>")]',
}
def classify(d,original):
    if d['name']not in NAMES:return None
    candidate=copy.deepcopy(d);candidate['captures']=[]
    human=original(candidate);assert human is not None,d['name']
    human['capture']=NAMES[d['name']]
    human.pop('births',None)
    return human

def migrate(root,baseline,output,apply=False):
    original=m.simple_facts
    def facts(d):return classify(d,original)
    m.simple_facts=facts
    try:report=m.migrate(root,baseline,output,apply)
    finally:m.simple_facts=original
    assert len(report['placements'])==len(NAMES)
    return report
if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('baseline',type=Path);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path,required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args();migrate(a.root,json.loads(a.baseline.read_text()),a.output,a.apply)
