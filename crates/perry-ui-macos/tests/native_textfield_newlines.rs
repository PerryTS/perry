// TextField and SecureField hold one line, as a web `<input>` does:
// - a value set in code drops its line breaks;
// - typed, pasted or dropped text turns each line break into one space;
// - Option-Return and Control-Return insert nothing;
// - Return still submits.
//
// AppKit must run on the process main thread, so this test has no Rust harness.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn js_closure_alloc(func_ptr: *const u8, capture_count: u32) -> *mut u8;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Sel};
    use objc2::{msg_send, sel, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSPasteboard, NSTextField, NSWindow, NSWindowStyleMask,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{MainThreadMarker, NSRange, NSString};
    use perry_ui_macos::widgets;

    /// How the case puts text into the field.
    #[derive(Debug, Clone, Copy)]
    enum Input {
        SetInCode(&'static str),
        Typed(&'static str),
        Pasted(&'static str),
        Dropped(&'static str),
        Command(Sel),
    }

    /// What the field shows once the input is in.
    #[derive(Debug, PartialEq)]
    struct Outcome {
        value: String,
        submitted: bool,
        height: f64,
    }

    static SUBMITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    if std::env::args().any(|arg| arg == "--list") {
        println!("native_textfield_newlines: test");
        return;
    }
    let mtm = MainThreadMarker::new().expect("TextField newline test runs on the main thread");
    let _app = NSApplication::sharedApplication(mtm);

    let mut failures = Vec::new();
    for secure in [false, true] {
        let one_line = run(secure, Input::SetInCode("ab"), mtm).height;
        // Each case: the input, the value the field ends with, and whether it
        // submits.
        let cases = [
            (Input::SetInCode("a\nb\r\nc\rd"), "abcd", false),
            (Input::Typed("a\nb"), "abca b", false),
            (Input::Pasted("a\nb\r\nc"), "abca b c", false),
            (Input::Dropped("a\nb"), "abca b", false),
            (
                Input::Command(sel!(insertNewlineIgnoringFieldEditor:)),
                "abc",
                false,
            ),
            (Input::Command(sel!(insertLineBreak:)), "abc", false),
            (
                Input::Command(sel!(insertParagraphSeparator:)),
                "abc",
                false,
            ),
            (Input::Command(sel!(insertNewline:)), "abc", true),
        ];
        for (input, value, submits) in cases {
            let expected = Outcome {
                value: value.to_string(),
                // Perry exposes onSubmit on TextField only.
                submitted: submits && !secure,
                height: one_line,
            };
            let actual = run(secure, input, mtm);
            let name = format!(
                "{} {input:?}",
                if secure { "SecureField" } else { "TextField" }
            );
            println!("{name}: {actual:?}");
            if actual != expected {
                failures.push(format!("{name}: expected {expected:?}, got {actual:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    println!("PASS native TextField newlines");

    /// Puts `input` into a new field that holds "abc" with the caret at its
    /// end, then reads back what the field holds.
    fn run(secure: bool, input: Input, mtm: MainThreadMarker) -> Outcome {
        // An edit calls onChange, so the field needs a real closure.
        extern "C" fn on_change(_closure: *const u8, _value: f64) -> f64 {
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        extern "C" fn on_submit(_closure: *const u8, _value: f64) -> f64 {
            SUBMITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
        let closure = |func: extern "C" fn(*const u8, f64) -> f64| unsafe {
            js_nanbox_pointer(js_closure_alloc(func as *const u8, 0) as i64)
        };
        let on_change = closure(on_change);
        let empty = perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
        let handle = if secure {
            widgets::securefield::create(empty.cast(), on_change)
        } else {
            widgets::textfield::create(empty.cast(), on_change)
        };
        if !secure {
            widgets::textfield::set_on_submit(handle, closure(on_submit));
        }
        let submits_before = SUBMITS.load(std::sync::atomic::Ordering::SeqCst);
        let view = widgets::get_widget(handle).unwrap();
        let field = unsafe { &*(Retained::as_ptr(&view) as *const NSTextField) };
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(300.0, 80.0)),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.contentView().unwrap().addSubview(&view);

        if let Input::SetInCode(text) = input {
            widgets::textfield::set_text_str(handle, text);
        } else {
            widgets::textfield::set_text_str(handle, "abc");
            window.makeFirstResponder(Some(field));
            let editor = field
                .currentEditor()
                .expect("the focused field has an editor");
            let editor: &AnyObject = &editor;
            unsafe {
                let _: () = msg_send![editor, setSelectedRange: NSRange::new(3, 0)];
                match input {
                    Input::Typed(text) => {
                        let _: () = msg_send![editor, insertText: &*NSString::from_str(text), replacementRange: NSRange::new(3, 0)];
                    }
                    Input::Pasted(text) | Input::Dropped(text) => {
                        let pasteboard =
                            NSPasteboard::pasteboardWithName(&NSString::from_str("perry-newlines"));
                        pasteboard.clearContents();
                        let string_type = NSString::from_str("public.utf8-plain-text");
                        pasteboard.setString_forType(&NSString::from_str(text), &string_type);
                        // A drop reads the dragging pasteboard by type; a paste
                        // reads the general one without.
                        let _: bool = if matches!(input, Input::Dropped(_)) {
                            msg_send![editor, readSelectionFromPasteboard: &*pasteboard, type: &*string_type]
                        } else {
                            msg_send![editor, readSelectionFromPasteboard: &*pasteboard]
                        };
                    }
                    Input::Command(command) => {
                        let _: () = msg_send![editor, doCommandBySelector: command];
                    }
                    Input::SetInCode(_) => unreachable!(),
                }
            }
        }
        let submitted = SUBMITS.load(std::sync::atomic::Ordering::SeqCst) != submits_before;
        let value = match field.currentEditor() {
            Some(editor) => editor.string().to_string(),
            None => field.stringValue().to_string(),
        };
        // End editing so the field's own value and size reflect the input.
        window.makeFirstResponder(None);
        let outcome = Outcome {
            value,
            submitted,
            height: field.intrinsicContentSize().height,
        };
        assert_eq!(
            field.stringValue().to_string(),
            outcome.value,
            "the field keeps the editor's value once editing ends"
        );
        window.close();
        outcome
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
