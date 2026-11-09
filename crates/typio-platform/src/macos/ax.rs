//! Thin safe wrappers over `AXUIElement` attribute calls.

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};

use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{
    CFBoolean, CFRange, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};

use crate::Rect;

#[derive(Debug, Clone)]
pub struct Element(pub CFRetained<AXUIElement>);

impl Element {
    pub fn system_wide() -> Self {
        let element = unsafe { AXUIElement::new_system_wide() };
        // A hung app must not freeze typing everywhere else.
        unsafe { element.set_messaging_timeout(0.25) };
        Self(element)
    }

    pub fn application(pid: i32) -> Self {
        Self(unsafe { AXUIElement::new_application(pid) })
    }

    pub fn attribute(&self, name: &str) -> Option<CFRetained<CFType>> {
        let name = CFString::from_str(name);
        let mut value: *const CFType = ptr::null();
        let error = unsafe {
            self.0
                .copy_attribute_value(&name, NonNull::from(&mut value))
        };
        let value = NonNull::new(value.cast_mut())?;
        // Copy functions return +1 references, owned from here on.
        let value = unsafe { CFRetained::from_raw(value) };
        (error == AXError::Success).then_some(value)
    }

    /// The raw `AXError` code reading `name` returns, for diagnostics.
    pub fn error_code(&self, name: &str) -> i32 {
        let name = CFString::from_str(name);
        let mut value: *const CFType = ptr::null();
        let error = unsafe {
            self.0
                .copy_attribute_value(&name, NonNull::from(&mut value))
        };
        if let Some(value) = NonNull::new(value.cast_mut()) {
            drop(unsafe { CFRetained::from_raw(value) });
        }
        error.0
    }

    pub fn parameterized(&self, name: &str, parameter: &CFType) -> Option<CFRetained<CFType>> {
        let name = CFString::from_str(name);
        let mut value: *const CFType = ptr::null();
        let error = unsafe {
            self.0
                .copy_parameterized_attribute_value(&name, parameter, NonNull::from(&mut value))
        };
        let value = NonNull::new(value.cast_mut())?;
        let value = unsafe { CFRetained::from_raw(value) };
        (error == AXError::Success).then_some(value)
    }

    pub fn string(&self, name: &str) -> Option<String> {
        let value = self.attribute(name)?.downcast::<CFString>().ok()?;
        Some(value.to_string())
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        let value = self.attribute(name)?.downcast::<CFBoolean>().ok()?;
        Some(value.as_bool())
    }

    pub fn element(&self, name: &str) -> Option<Element> {
        let value = self.attribute(name)?.downcast::<AXUIElement>().ok()?;
        Some(Element(value))
    }

    pub fn range(&self, name: &str) -> Option<CFRange> {
        read_value(&*self.attribute(name)?, AXValueType::CFRange)
    }

    pub fn rect_for_range(&self, range: CFRange) -> Option<Rect> {
        let parameter = make_value(AXValueType::CFRange, range)?;
        let value = self.parameterized("AXBoundsForRange", &parameter)?;
        read_value::<CGRect>(&value, AXValueType::CGRect).map(to_rect)
    }

    pub fn frame(&self) -> Option<Rect> {
        let origin: CGPoint = read_value(&*self.attribute("AXPosition")?, AXValueType::CGPoint)?;
        let size: CGSize = read_value(&*self.attribute("AXSize")?, AXValueType::CGSize)?;
        Some(to_rect(CGRect { origin, size }))
    }

    pub fn pid(&self) -> Option<i32> {
        let mut pid = 0;
        let error = unsafe { self.0.pid(NonNull::from(&mut pid)) };
        (error == AXError::Success).then_some(pid)
    }

    pub fn set(&self, name: &str, value: &CFType) -> bool {
        let name = CFString::from_str(name);
        unsafe { self.0.set_attribute_value(&name, value) == AXError::Success }
    }

    pub fn set_string(&self, name: &str, value: &str) -> bool {
        self.set(name, &CFString::from_str(value))
    }

    pub fn set_bool(&self, name: &str, value: bool) -> bool {
        self.set(name, CFBoolean::new(value))
    }

    pub fn set_range(&self, name: &str, range: CFRange) -> bool {
        make_value(AXValueType::CFRange, range).is_some_and(|value| self.set(name, &value))
    }
}

fn make_value<T>(kind: AXValueType, mut value: T) -> Option<CFRetained<CFType>> {
    let pointer = NonNull::from(&mut value).cast::<c_void>();
    let value = unsafe { AXValue::new(kind, pointer) }?;
    Some(value.into())
}

fn read_value<T>(value: &CFType, kind: AXValueType) -> Option<T> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut out = MaybeUninit::<T>::uninit();
    let ok = unsafe { value.value(kind, NonNull::from(&mut out).cast()) };
    // SAFETY: AXValueGetValue filled `out` with a `kind` value, which is a `T`.
    ok.then(|| unsafe { out.assume_init() })
}

fn to_rect(rect: CGRect) -> Rect {
    Rect {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}
