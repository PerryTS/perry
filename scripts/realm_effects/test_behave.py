#!/usr/bin/env python3
"""Unit tests of the behavioural effect check's comparator and generator
(scripts/realm_effects/behave.py). No perry needed: run
`python3 scripts/realm_effects/test_behave.py -v`."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import behave  # noqa: E402

THIS = {"root": "this", "steps": []}
ELEMS = {"root": "this", "steps": ["elems"]}
ARG0 = {"root": {"arg": 0}, "steps": []}
ARG1 = {"root": {"arg": 1}, "steps": []}
RET0 = {"root": {"returned": 0}, "steps": []}

CALLS = {"calls": {"callee": ARG0, "this": {"slot": ARG1},
                   "args": [{"slot": ELEMS}, {"prim": ["num"]}, {"slot": THIS}]}}
JOIN = {"name": "X.prototype.join", "effects": [
    {"reads": [THIS]},
    {"coerce": {"protocol": "to_primitive", "slot": ELEMS}},
    {"coerce": {"protocol": "to_primitive", "slot": ARG0}},
    "stores_fresh", {"returns": {"prim": ["str"]}}]}
MAP_FRESH = {"name": "X.prototype.map", "effects": [
    {"reads": [THIS]}, CALLS,
    {"returns": {"fresh": {"proto": "Array.prototype", "elems": [RET0]}}}]}
MAP_SPECIES = {"name": "X.prototype.map", "effects": [
    {"reads": [THIS]}, CALLS,
    {"returns": {"species": {"of": THIS, "args": [{"prim": ["num"]}], "elems": [RET0]}}}]}
PUSH = {"name": "X.prototype.push", "effects": [
    {"reads": [THIS]},
    {"stores": {"src": {"root": {"rest": 0}, "steps": []}, "dst": ELEMS}},
    {"returns": {"prim": ["num"]}}]}

SPECIES_RUN = """EV args 2
EV trap this get constructor
EV trap field(this,constructor) get @@species
EV construct field(field(this,constructor),@@species) args=prim:num
EV call arg(0) this=arg(1)#0 args=elems(this)#0,prim:num,this
EV trap species(this)#0 defineProperty 0 returned(0)#0
EV result species(this)#0
EV end
"""


def reds(decl, text):
    return behave.check(decl, text)[0]


class Slots(unittest.TestCase):
    def test_slot_text_is_the_grammar(self):
        self.assertEqual(behave.slot_text(ELEMS), "elems(this)")
        self.assertEqual(behave.slot_text(RET0), "returned(0)")
        self.assertEqual(behave.slot_text({"root": "this", "steps": [{"field": "constructor"}]}),
                         "field(this,constructor)")
        self.assertEqual(behave.root_of("elems(field(arg(1),x))"), "arg(1)")

    def test_call_shape_comes_from_the_declaration(self):
        # map names arg(0) and arg(1); one argument more is passed.
        shape = behave.shape_of(MAP_FRESH)
        self.assertEqual((shape["receiver"], shape["nargs"], shape["callees"]), (True, 3, {0: 0}))
        shape = behave.shape_of(dict(PUSH, length=1))
        self.assertEqual((shape["nargs"], shape["rest"]), (0, 0))
        # map runs with the callback alone and with the thisArg too.
        self.assertIn(("plain", "method", 1), behave.scenarios(MAP_FRESH))
        self.assertIn(("proxy", "apply", 2), behave.scenarios(MAP_FRESH))
        self.assertNotIn(("plain", "apply", 0), behave.scenarios(MAP_FRESH))

    def test_an_argument_the_declaration_omits_is_still_passed(self):
        # The sabotage drops join's coerce effects, so no effect names arg(0):
        # its `length` and the extra argument still pass a probe there.
        weak = dict(JOIN, length=1, effects=behave.weaken(JOIN["effects"], "coerce"))
        src = behave.generate(weak, "plain", "apply")
        self.assertIn('value("arg(0)#0")', src)
        self.assertIn('value("arg(1)#0")', src)
        # A prototype method always gets the probe receiver.
        self.assertTrue(behave.shape_of(dict(weak, effects=[{"returns": {"prim": ["str"]}}]))["receiver"])

    def test_generated_program_calls_the_native_by_its_path(self):
        src = behave.generate(MAP_FRESH, "species", "method", 1)
        self.assertIn('["X", "prototype", "map"]', src)
        self.assertIn("recv.map(args[0])", src)
        self.assertIn('callback("arg(0)", 0)', src)
        self.assertNotIn('value("arg(1)#0")', src)
        self.assertIn('"use strict";', src)
        self.assertIn("const a0 = args[0]; return recv.map(a0)", behave.generate(MAP_FRESH, "plain", "local", 1))


class Mapping(unittest.TestCase):
    def test_coerce_of_a_declared_slot_is_covered(self):
        self.assertEqual(reds(JOIN, "EV args 1\nEV coerce elems(this)#0 string\nEV result prim:str\nEV end\n"), [])

    def test_sabotage_a_weakened_declaration_goes_red_naming_the_effect(self):
        weak = dict(JOIN, effects=behave.weaken(JOIN["effects"], "coerce"))
        r = reds(weak, "EV args 1\nEV coerce elems(this)#0 string\nEV result prim:str\nEV end\n")
        self.assertEqual(r, ["coerce elems(this)#0 string needs coerce(elems(this), ToPrimitive)"])

    def test_a_get_of_toprimitive_is_a_read_or_the_coerce(self):
        self.assertEqual(reds(JOIN, "EV trap arg(0)#0 get @@toPrimitive\nEV end\n"), [])
        r = reds(JOIN, "EV trap arg(0)#0 get foo\nEV end\n")
        self.assertEqual(r, ["trap arg(0)#0 get foo needs reads(arg(0))"])

    def test_species_construction_is_red_against_a_fresh_declaration(self):
        r = reds(MAP_FRESH, SPECIES_RUN)
        self.assertIn("trap field(this,constructor) get @@species needs reads(field(this,constructor))", r)
        self.assertIn("construct field(field(this,constructor),@@species) args=prim:num "
                      "needs returns(species(this, args = [prim:num]))", r)
        self.assertIn("result species(this)#0 needs returns(species(this))", r)

    def test_species_construction_is_covered_by_a_species_declaration(self):
        self.assertEqual(reds(MAP_SPECIES, SPECIES_RUN), [])
        # The default constructor makes a plain array: species covers it too.
        self.assertEqual(reds(MAP_SPECIES, "EV result obj:Array.prototype\n"
                                           "EV result-elem 0 returned(0)#0\nEV end\n"), [])

    def test_a_result_element_must_be_declared(self):
        r = reds(MAP_FRESH, "EV result obj:Array.prototype\nEV result-elem 0 elems(this)#0\nEV end\n")
        self.assertEqual(r, ["result-elem 0 elems(this)#0 needs returns(.. elems = [elems(this)])"])

    def test_a_callback_call_needs_its_this_and_args_covered(self):
        ok = "EV args 2\nEV call arg(0) this=arg(1)#0 args=elems(this)#0,prim:num,this\nEV end\n"
        self.assertEqual(reds(MAP_FRESH, ok), [])
        # thisArg not passed: the callback sees undefined, which arg(1) covers.
        self.assertEqual(reds(MAP_FRESH, ok.replace("args 2", "args 1").replace("arg(1)#0", "prim:undefined")), [])
        # An extra argument the declaration does not name is red.
        extra = ok.replace("prim:num,this", "prim:num,this,prim:str")
        self.assertEqual(len(reds(MAP_FRESH, extra)), 1)
        # A this the declaration does not name is red.
        self.assertEqual(len(reds(MAP_FRESH, ok.replace("this=arg(1)#0", "this=obj:globalThis"))), 1)

    def test_stores_into_the_receiver(self):
        run = "EV args 0\nEV trap this set 3 rest(0)#0\nEV trap this set length prim:num\nEV alive rest(0)#0 in=this\nEV end\n"
        self.assertEqual(reds(PUSH, run), [])
        r = reds(JOIN, run)
        self.assertEqual(r[0], "trap this set 3 rest(0)#0 needs stores(rest(0) -> elems(this))")

    def test_invisible_retention_needs_open(self):
        r = reds(JOIN, "EV alive arg(0)#0 in=none\nEV end\n")
        self.assertEqual(r, ["alive arg(0)#0 in=none needs open (arg(0) retained where the program cannot see it)"])
        self.assertEqual(reds(dict(JOIN, effects=["open"]), "EV alive arg(0)#0 in=none\nEV end\n"), [])

    def test_a_program_that_did_not_finish_is_red(self):
        self.assertEqual(reds(JOIN, "EV args 1\n"), ["program did not finish (no `end` event)"])


if __name__ == "__main__":
    unittest.main()
