//! Colours in sRGB. `perry/ui` RGBA components are sRGB. If they went to
//! `CGColorCreateGenericRGB` or `colorWithCalibratedRed:` instead, they would be
//! read as Generic RGB, and `#00C2FF` would render as `#00CDFF`.

use objc2::rc::Retained;
use objc2_app_kit::NSColor;
use std::ffi::c_void;
use std::sync::OnceLock;

/// CoreGraphics' opaque colour. A typed pointer encodes as `^{CGColor=}`, the
/// type CALayer's colour properties declare. If the pointer were `c_void`, it
/// would encode as `^v`, and a debug build's message check would reject the send.
#[repr(C)]
pub struct CGColor {
    _private: [u8; 0],
}

unsafe impl objc2::encode::RefEncode for CGColor {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("CGColor", &[]));
}

type CGColorRef = *mut CGColor;
type CGColorSpaceRef = *mut c_void;

extern "C" {
    static kCGColorSpaceSRGB: *const c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> CGColorSpaceRef;
    fn CGColorCreate(space: CGColorSpaceRef, components: *const f64) -> CGColorRef;
    fn CGColorRelease(color: CGColorRef);
    fn CGContextSetFillColorSpace(c: *mut c_void, space: CGColorSpaceRef);
    fn CGContextSetFillColor(c: *mut c_void, components: *const f64);
    fn CGContextSetStrokeColorSpace(c: *mut c_void, space: CGColorSpaceRef);
    fn CGContextSetStrokeColor(c: *mut c_void, components: *const f64);
}

/// The sRGB colour space. Created once and held for the life of the process.
pub fn color_space() -> CGColorSpaceRef {
    static SPACE: OnceLock<usize> = OnceLock::new();
    *SPACE.get_or_init(|| unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) as usize })
        as CGColorSpaceRef
}

/// An sRGB `NSColor`.
pub fn ns_color(r: f64, g: f64, b: f64, a: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a)
}

/// An owned sRGB `CGColor`, released on drop.
pub struct CgColor(CGColorRef);

impl CgColor {
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        let components = [r, g, b, a];
        Self(unsafe { CGColorCreate(color_space(), components.as_ptr()) })
    }

    pub fn as_ptr(&self) -> CGColorRef {
        self.0
    }
}

impl Drop for CgColor {
    fn drop(&mut self) {
        unsafe { CGColorRelease(self.0) }
    }
}

/// Sets the fill colour from sRGB components without allocating a `CGColor`.
///
/// # Safety
/// `ctx` must be a valid `CGContextRef`.
pub unsafe fn set_fill_color(ctx: *mut c_void, r: f64, g: f64, b: f64, a: f64) {
    CGContextSetFillColorSpace(ctx, color_space());
    CGContextSetFillColor(ctx, [r, g, b, a].as_ptr());
}

/// Sets the stroke colour from sRGB components without allocating a `CGColor`.
///
/// # Safety
/// `ctx` must be a valid `CGContextRef`.
pub unsafe fn set_stroke_color(ctx: *mut c_void, r: f64, g: f64, b: f64, a: f64) {
    CGContextSetStrokeColorSpace(ctx, color_space());
    CGContextSetStrokeColor(ctx, [r, g, b, a].as_ptr());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::CGContextFillRect;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    extern "C" {
        static kCGColorSpaceDisplayP3: *const c_void;
        fn CGColorGetColorSpace(color: CGColorRef) -> CGColorSpaceRef;
        fn CGColorSpaceCopyName(space: CGColorSpaceRef) -> *const c_void;
        fn CGColorSpaceRelease(space: CGColorSpaceRef);
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: CGColorSpaceRef,
            bitmap_info: u32,
        ) -> *mut c_void;
        fn CGContextRelease(c: *mut c_void);
        fn CGContextSetFillColorWithColor(c: *mut c_void, color: CGColorRef);
        fn CGContextStrokeRectWithWidth(c: *mut c_void, rect: CGRect, width: f64);
        fn CFEqual(a: *const c_void, b: *const c_void) -> u8;
        fn CFRelease(obj: *const c_void);
    }

    /// `#00C2FF`.
    const CYAN: [f64; 4] = [0.0, 194.0 / 255.0, 1.0, 1.0];
    const PREMULTIPLIED_LAST: u32 = 1;

    fn is_srgb(color: CGColorRef) -> bool {
        unsafe {
            let name = CGColorSpaceCopyName(CGColorGetColorSpace(color));
            if name.is_null() {
                return false;
            }
            let equal = CFEqual(name, kCGColorSpaceSRGB) != 0;
            CFRelease(name);
            equal
        }
    }

    /// One pixel of a Display P3 bitmap, where Generic RGB and sRGB components
    /// land on different bytes.
    fn paint(draw: impl FnOnce(*mut c_void)) -> [u8; 4] {
        let mut pixel = [0u8; 4];
        unsafe {
            let space = CGColorSpaceCreateWithName(kCGColorSpaceDisplayP3);
            let ctx = CGBitmapContextCreate(
                pixel.as_mut_ptr().cast(),
                1,
                1,
                8,
                4,
                space,
                PREMULTIPLIED_LAST,
            );
            draw(ctx);
            CGContextRelease(ctx);
            CGColorSpaceRelease(space);
        }
        pixel
    }

    #[test]
    fn constructors_make_srgb_colors() {
        let [r, g, b, a] = CYAN;
        assert!(is_srgb(CgColor::new(r, g, b, a).as_ptr()));
        let ns = ns_color(r, g, b, a).CGColor();
        assert!(is_srgb(&*ns as *const _ as CGColorRef));
    }

    #[test]
    fn context_setters_paint_srgb() {
        let [r, g, b, a] = CYAN;
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1.0, 1.0));
        let color = CgColor::new(r, g, b, a);
        let expected = paint(|ctx| unsafe {
            CGContextSetFillColorWithColor(ctx, color.as_ptr());
            CGContextFillRect(ctx, rect);
        });
        let fill = paint(|ctx| unsafe {
            set_fill_color(ctx, r, g, b, a);
            CGContextFillRect(ctx, rect);
        });
        let stroke = paint(|ctx| unsafe {
            set_stroke_color(ctx, r, g, b, a);
            CGContextStrokeRectWithWidth(ctx, rect, 4.0);
        });
        assert_eq!([fill, stroke], [expected; 2]);
    }
}
