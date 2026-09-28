//! CoreGraphics colours in sRGB. A `perry/ui` RGBA value means sRGB on every
//! target, as CSS reads it on the web. `CGColorCreateGenericRGB` and
//! `CGContextSetRGB{Fill,Stroke}Color` would read the same components as
//! Generic RGB or device RGB, so `#00C2FF` would render as `#00CDFF`.

use std::ffi::c_void;
use std::sync::OnceLock;

type CGColorRef = *mut c_void;
type CGColorSpaceRef = *mut c_void;

extern "C" {
    static kCGColorSpaceSRGB: *const c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> CGColorSpaceRef;
    fn CGColorCreate(space: CGColorSpaceRef, components: *const f64) -> CGColorRef;
    fn CGColorRelease(color: CGColorRef);
    fn CGContextSetFillColorWithColor(c: *mut c_void, color: CGColorRef);
    fn CGContextSetStrokeColorWithColor(c: *mut c_void, color: CGColorRef);
}

/// The sRGB colour space. Created once and held for the life of the process.
pub fn color_space() -> CGColorSpaceRef {
    static SPACE: OnceLock<usize> = OnceLock::new();
    *SPACE.get_or_init(|| unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) as usize })
        as CGColorSpaceRef
}

/// An owned sRGB `CGColor`, released on drop. CoreAnimation retains the colour
/// it is given, so the owner can drop it right after the setter returns.
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

/// # Safety
/// `ctx` must be a valid `CGContextRef`.
pub unsafe fn set_fill_color(ctx: *mut c_void, r: f64, g: f64, b: f64, a: f64) {
    CGContextSetFillColorWithColor(ctx, CgColor::new(r, g, b, a).as_ptr());
}

/// # Safety
/// `ctx` must be a valid `CGContextRef`.
pub unsafe fn set_stroke_color(ctx: *mut c_void, r: f64, g: f64, b: f64, a: f64) {
    CGContextSetStrokeColorWithColor(ctx, CgColor::new(r, g, b, a).as_ptr());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    extern "C" {
        fn CGColorGetColorSpace(color: CGColorRef) -> CGColorSpaceRef;
        fn CGColorSpaceCopyName(space: CGColorSpaceRef) -> *const c_void;
        fn CFEqual(a: *const c_void, b: *const c_void) -> u8;
        fn CFRelease(obj: *const c_void);
    }

    pub(crate) fn is_srgb(color: CGColorRef) -> bool {
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

    #[test]
    fn cg_color_is_srgb() {
        assert!(is_srgb(CgColor::new(0.0, 194.0 / 255.0, 1.0, 1.0).as_ptr()));
    }
}
