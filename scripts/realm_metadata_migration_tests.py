#!/usr/bin/env python3
import unittest
from realm_metadata_migration import birth_literal, human_length, temporal_rows
from realm_source_migration_tests import atomics_add


class PublicLengthMigrationTests(unittest.TestCase):
    def test_only_semantic_override_survives(self):
        d=atomics_add()
        d["length"]=2
        d["properties"][0]["value"]["value"]="4000000000000000"
        self.assertEqual(human_length(d),({},2))
        d["captures"]=[{"kind":"raw_word","value":"0x1"}]
        self.assertIsNone(human_length(d))

    def test_birth_literal_is_outside_the_info_abi(self):
        source='method("x", fn_info!(body,3;with_declared(3)), 3, 2, true)'
        site=dict(offset=source.index("body"))
        self.assertEqual(birth_literal(source,site,2)[0],"2")
        self.assertIsNone(birth_literal(source,site,0))
        source='method("x", fn_info!(body,3), length)'
        self.assertIsNone(birth_literal(source,dict(offset=source.index("body")),2))

    def test_temporal_births_are_qualified_by_the_declared_owner(self):
        source='populate_prototype(date,"Temporal.PlainDate",&["year"],&[("add",1),("toString",0)]);'
        rows=temporal_rows(source)
        self.assertEqual(rows["Temporal.PlainDate.prototype.add"][0],"1")
        self.assertEqual(len(rows),2)

    def test_repeated_equal_arguments_are_not_guessed(self):
        source='method("x", fn_info!(body,3), 1, 1)'
        self.assertIsNone(birth_literal(source,dict(offset=source.index("body")),1))


class ConstructorLengthMigrationTests(unittest.TestCase):
    def test_shared_match_arm_retains_one_literal(self):
        from realm_metadata_migration import constructor_lengths
        rows=constructor_lengths('fn builtin_constructor_spec_length(name:&str)->Option<u32> { let n=match name {"A"|"B"=>0,"C"=>2,_=>return None}; Some(n) }')
        self.assertEqual(set(rows),{"A","B","C"})
        self.assertEqual(rows["A"],rows["B"])
        self.assertEqual(rows["C"][0],"2")


    def test_shared_constructor_literal_is_replaced_once_across_body_sites(self):
        import tempfile
        import json
        from pathlib import Path
        from unittest.mock import patch
        import realm_metadata_migration as migration
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            file=root/"crates/perry-runtime/src/object/global_this_tables.rs"
            file.parent.mkdir(parents=True)
            source='const A_INFO=fn_info!(a,1);\nconst B_INFO=fn_info!(b,1);\nfn builtin_constructor_spec_length(name:&str)->Option<u32>{let n=match name {"A"|"B"=>0,_=>return None};Some(n)}\n'
            file.write_text(source)
            info=dict(params=1,rest_fixed=0,flags=0,length=0,declared=0,plain_params=0)
            entries=[dict(name=name,kind="constructor",body=name.lower(),length=0,function_info=info)for name in ("A","B")]
            file.with_suffix(".realm.json").write_text(json.dumps(dict(version=1,groups=dict(cells=entries))))
            with file.open("a")as output:output.write('realm_decl!("global_this_tables.realm.json#*");\n')
            baseline=dict(declarations=[dict(d,source="perry-runtime/src/object/global_this_tables.rs",line=4)for d in entries])
            with patch.object(migration.migration,"abi_constants",return_value={}),patch.object(migration.migration,"constructor_families",return_value={}),patch.object(migration,"human_length",return_value=({"kind":"constructor"},0)):
                migration.migrate(root,baseline,root/"report",apply=True,constructors=True)
            text=file.read_text()
            report=json.loads((root/"report/migration-public-length.json").read_text())
            names={d["projection"]for d in report["placements"]}
            self.assertEqual(len(names),1)
            macro=names.pop()
            self.assertEqual(text.count(macro+"!(length)"),1)
            self.assertEqual(text.count("metadata("+macro+")"),2)
            self.assertIn('=>'+macro+'!(length),',text)

class MemberLengthMigrationTests(unittest.TestCase):
    def test_only_literal_lengths_of_the_selected_table_are_migrated(self):
        from realm_metadata_migration import member_lengths
        source='const MEMBERS:&[(&str,u32)]=&[("a",1),("b",existing_macro!())];'
        rows=member_lengths(source,"MEMBERS","X.prototype")
        self.assertEqual(set(rows),{"X.prototype.a"})
        self.assertEqual(rows["X.prototype.a"][0],"1")
        with self.assertRaises(ValueError):member_lengths(source,"MISSING","X")


class HttpLengthMigrationTests(unittest.TestCase):
    def test_each_owner_selects_its_own_literal(self):
        from realm_metadata_migration import http_lengths
        source='fn build_http_prototype(){match kind{HttpProto::A=>{method("close",0);for (name,length) in [("write",2)]{method(name,length);}},HttpProto::B=>{method("close",1);}}}'
        rows=http_lengths(source)
        self.assertEqual({k:v[0]for k,v in rows.items()},{"node:http.A.prototype.close":"0","node:http.A.prototype.write":"2","node:http.B.prototype.close":"1"})


class HttpNameMigrationTests(unittest.TestCase):
    def test_accessor_halves_share_the_runtime_key_literal(self):
        from realm_metadata_migration import http_accessor_labels
        source='fn build_http_prototype(){match kind{HttpProto::A=>{accessor("both",true);for name in ["read"]{accessor(name,false);}}}}'
        rows=http_accessor_labels(source)
        self.assertEqual(set(rows),{"node:http.A.prototype.both.<get>","node:http.A.prototype.both.<set>","node:http.A.prototype.read.<get>"})
        self.assertEqual(rows["node:http.A.prototype.both.<get>"],rows["node:http.A.prototype.both.<set>"])


if __name__=="__main__":
    unittest.main()
