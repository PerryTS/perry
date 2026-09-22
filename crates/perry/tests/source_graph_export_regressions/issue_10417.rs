use super::{compile_and_run, write};

#[test]
fn enum_members_are_not_copied_into_importing_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "utils.ts",
        "enum CharacterCode { LowerA = 97, LowerZ = 122, Lt = 60 }\n\
         export function isHtml(str: string): boolean {\n\
           const c = str.charCodeAt(1);\n\
           return str.charCodeAt(0) === CharacterCode.Lt &&\n\
             c >= CharacterCode.LowerA && c <= CharacterCode.LowerZ;\n\
         }\n\
         export class Probe {\n\
           isLt(str: string): boolean {\n\
             return str.charCodeAt(0) === CharacterCode.Lt;\n\
           }\n\
         }\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { isHtml, Probe } from './utils';\n\
         console.log(isHtml('<div>'), isHtml('hello'));\n\
         const probe = new Probe();\n\
         console.log(probe.isLt('<a'), probe.isLt('a'));\n",
    );

    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "true false\ntrue false\n"
    );
}
