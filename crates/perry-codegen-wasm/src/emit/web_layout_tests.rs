//! perry/ui layout and widget calls on the web target, measured against what the
//! macOS backend does with the same call (docs/src/ui/layout.md).

use super::run_web_ui_source_then;

// Stack alignment is an NSLayoutAttribute, as NSStackView takes it on macOS.
#[test]
fn stack_alignment_takes_layout_attribute_values() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { VStack, HStack, stackSetAlignment } from "perry/ui";
const values = [5, 9, 7, 3, 10, 4];
for (let i = 0; i < values.length; i++) {
  const s = i < 3 ? VStack(0, []) : HStack(0, []);
  stackSetAlignment(s, values[i]);
}
"#,
            r#"
console.log([...uiHandles.values()].filter(e => e.style.display === "flex").map(e => e.style.alignItems).join(" "));
"#
        ),
        "flex-start center stretch flex-start center flex-end"
    );
}

// A hugging priority below 250 stretches the widget, the lower the more; 250 and up
// keep its size.
#[test]
fn hugging_priority_decides_whether_a_widget_stretches() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { HStack, Text, widgetSetHugging } from "perry/ui";
const low = Text("low"), high = Text("high");
widgetSetHugging(low, 1);
widgetSetHugging(high, 750);
HStack(0, [low, high]);
"#,
            r#"
for (const label of ["low", "high"]) console.log(label + " " + [...uiHandles.values()].find(e => e.textContent === label).style.flex);
"#
        ),
        "low 249 1 0%\nhigh 0 0 auto"
    );
}

// FillEqually gives every child, present and later, an equal share; under Fill a
// row's child stack without its own hugging priority takes the free width, and
// one with a high priority keeps its size.
#[test]
fn distribution_and_fill_share_a_rows_width() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { HStack, VStack, Text, widgetAddChild, widgetSetHugging, stackSetDistribution } from "perry/ui";
const equal = HStack(0, [Text("e1")]);
stackSetDistribution(equal, 1);
widgetAddChild(equal, Text("e2"));
const inner = HStack(0, [Text("inner")]);
const pinned = HStack(0, [Text("pinned")]);
widgetSetHugging(pinned, 750);
HStack(0, [inner, pinned]);
"#,
            r#"
// e1 and e2 are the Texts themselves; inner and pinned are the stacks around their Text.
const by = (label, stack) => [...uiHandles.values()].find(e => e && e.style && e.textContent === label && (e.style.display === "flex") === stack);
console.log([["e1", false], ["e2", false], ["inner", true], ["pinned", true]].map(([l, stack]) => l + "=" + by(l, stack).style.flex).join(" "));
"#
        ),
        "e1=1 1 0% e2=1 1 0% inner=1 1 auto pinned=0 0 auto"
    );
}

// widgetSetOnClick makes any widget a button: click, Enter and Space run it.
#[test]
fn widget_set_on_click_runs_on_click_and_keyboard() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { HStack, Text, widgetSetOnClick } from "perry/ui";
let taps = 0;
const box = HStack(0, [Text("tap")]);
widgetSetOnClick(box, () => { taps++; console.log("tapped " + taps); });
"#,
            r#"
const box = [...uiHandles.values()].find(e => e.textContent === "tap" && e.style.display === "flex");
box.dispatch("click");
for (const fn of box.listeners.keydown) fn({ key: "Enter", target: box, preventDefault() {} });
for (const fn of box.listeners.keydown) fn({ key: "a", target: box, preventDefault() {} });
console.log(box.style.cursor, box.role, box.tabIndex);
"#
        ),
        "tapped 1\ntapped 2\npointer button 0"
    );
}

// A symbol draws symbols/<name>.svg as a mask over the colour imageSetTint sets.
#[test]
fn image_symbol_masks_its_svg_with_the_tint() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { ImageSymbol, imageSetTint } from "perry/ui";
imageSetTint(ImageSymbol("chevron.right"), 1, 0, 0, 1);
"#,
            r#"
const el = [...uiHandles.values()].find(e => e.style.maskImage);
console.log(el.style.maskImage, el.style.backgroundColor, el.style.color);
"#
        ),
        r#"url("symbols/chevron.right.svg") currentColor rgba(255,0,0,1)"#
    );
}

// textSetNumberOfLines clamps to that many lines; 0 lifts the limit.
#[test]
fn text_set_number_of_lines_clamps() {
    assert_eq!(
        run_web_ui_source_then(
            r#"
import { Text, textSetNumberOfLines } from "perry/ui";
textSetNumberOfLines(Text("two"), 2);
const free = Text("free");
textSetNumberOfLines(free, 3);
textSetNumberOfLines(free, 0);
"#,
            r#"
for (const label of ["two", "free"]) console.log(label + " [" + [...uiHandles.values()].find(e => e.textContent === label).style.webkitLineClamp + "]");
"#
        ),
        "two [2]\nfree []"
    );
}
