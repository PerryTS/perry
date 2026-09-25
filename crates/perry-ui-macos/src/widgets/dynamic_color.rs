//! Appearance-adaptive custom colors. AppKit retains dynamic NSColors for text
//! and tint, but CALayers retain resolved CGColors. A zero-size, unarranged child
//! view observes inherited appearance changes and refreshes those layer values.
//! It owns only colors; its superview owns it, avoiding an observer/owner cycle.
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSAppearance, NSAppearanceCustomization, NSButton, NSColor, NSView};
use objc2_foundation::{MainThreadMarker, NSArray, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ptr::NonNull;

pub(super) fn rgba([r, g, b, a]: [f64; 4]) -> Retained<NSColor> {
    NSColor::colorWithRed_green_blue_alpha(r, g, b, a)
}

fn color(light: [f64; 4], dark: [f64; 4]) -> Retained<NSColor> {
    let light = rgba(light);
    let dark = rgba(dark);
    // Include the concrete accessibility variants: on macOS 26 a two-name
    // Aqua/DarkAqua lookup can classify high-contrast DarkAqua as Aqua.
    let dark_names = [
        "NSAppearanceNameDarkAqua",
        "NSAppearanceNameVibrantDark",
        "NSAppearanceNameAccessibilityHighContrastDarkAqua",
        "NSAppearanceNameAccessibilityHighContrastVibrantDark",
    ]
    .map(NSString::from_str);
    let light_names = [
        "NSAppearanceNameAqua",
        "NSAppearanceNameVibrantLight",
        "NSAppearanceNameAccessibilityHighContrastAqua",
        "NSAppearanceNameAccessibilityHighContrastVibrantLight",
    ]
    .map(NSString::from_str);
    let names = NSArray::from_retained_slice(
        &dark_names
            .iter()
            .chain(light_names.iter())
            .cloned()
            .collect::<Vec<_>>(),
    );
    let provider = RcBlock::new(move |appearance: NonNull<NSAppearance>| {
        let best = unsafe { appearance.as_ref() }.bestMatchFromAppearancesWithNames(&names);
        let is_dark = best.as_ref().is_some_and(|best| dark_names.contains(best));
        let selected = if is_dark { &dark } else { &light };
        // Both colors remain retained by the copied provider block throughout
        // the dynamic color's lifetime, including after this call returns.
        NonNull::from(&**selected)
    });
    unsafe { NSColor::colorWithName_dynamicProvider(None, &provider) }
}

#[derive(Default)]
pub struct AppearanceColors {
    background: RefCell<Option<Retained<NSColor>>>,
    border: RefCell<Option<Retained<NSColor>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "PerryAppearanceColors"]
    #[ivars = AppearanceColors]
    struct AppearanceView;

    impl AppearanceView {
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            unsafe { let _: () = msg_send![super(self), viewDidChangeEffectiveAppearance]; }
            self.refresh();
        }
        #[unsafe(method(viewDidMoveToWindow))]
        fn moved_to_window(&self) {
            unsafe { let _: () = msg_send![super(self), viewDidMoveToWindow]; }
            self.refresh();
        }
    }
);

impl AppearanceView {
    fn refresh(&self) {
        let Some(parent) = (unsafe { self.superview() }) else {
            return;
        };
        let background = self.ivars().background.borrow().clone();
        let border = self.ivars().border.borrow().clone();
        if background.is_none() && border.is_none() {
            return;
        }
        parent.setWantsLayer(true);
        let appearance = parent.effectiveAppearance();
        appearance.performAsCurrentDrawingAppearance(&RcBlock::new(|| unsafe {
            let layer: *mut objc2::runtime::AnyObject = msg_send![&*parent, layer];
            if layer.is_null() {
                return;
            }
            if let Some(color) = &background {
                let cg = color.CGColor();
                let _: () = msg_send![layer, setBackgroundColor: &*cg];
            }
            if let Some(color) = &border {
                let cg = color.CGColor();
                let _: () = msg_send![layer, setBorderColor: &*cg];
            }
        }));
    }
}

fn observer(view: &NSView) -> Option<Retained<AppearanceView>> {
    view.subviews()
        .into_iter()
        .find_map(|child| child.downcast::<AppearanceView>().ok())
}

fn set_layer(handle: i64, light: [f64; 4], dark: [f64; 4], border: bool) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    let observer = observer(&view).unwrap_or_else(|| {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let allocated = AppearanceView::alloc(mtm).set_ivars(AppearanceColors::default());
        let child: Retained<AppearanceView> = unsafe {
            msg_send![super(allocated), initWithFrame: NSRect::new(NSPoint::ZERO, NSSize::ZERO)]
        };
        // addSubview, not addArrangedSubview: no layout slot or widget handle.
        view.addSubview(&child);
        child
    });
    if border {
        *observer.ivars().border.borrow_mut() = Some(color(light, dark));
    } else {
        super::BG_COLOR_MAP.with(|cache| cache.borrow_mut().remove(&handle));
        *observer.ivars().background.borrow_mut() = Some(color(light, dark));
    }
    observer.refresh();
}

pub(super) fn clear_layer_color(handle: i64, border: bool) {
    if let Some(observer) = super::get_widget(handle).and_then(|view| observer(&view)) {
        if border {
            observer.ivars().border.borrow_mut().take();
        } else {
            observer.ivars().background.borrow_mut().take();
        }
        if observer.ivars().background.borrow().is_none()
            && observer.ivars().border.borrow().is_none()
        {
            observer.removeFromSuperview();
        }
    }
}

pub(super) fn refresh(handle: i64) {
    if let Some(observer) = super::get_widget(handle).and_then(|view| observer(&view)) {
        observer.refresh();
    }
}

pub fn set_background(handle: i64, light: [f64; 4], dark: [f64; 4]) {
    set_layer(handle, light, dark, false);
}
pub fn set_border(handle: i64, light: [f64; 4], dark: [f64; 4]) {
    set_layer(handle, light, dark, true);
}
pub fn set_text(handle: i64, light: [f64; 4], dark: [f64; 4]) {
    super::text::set_ns_color(handle, &color(light, dark));
}
pub fn set_button_tint(handle: i64, light: [f64; 4], dark: [f64; 4]) {
    if let Some(view) = super::get_widget(handle) {
        if let Some(button) = view.downcast_ref::<NSButton>() {
            button.setContentTintColor(Some(&color(light, dark)));
        }
    }
}
