//! Making the focused text field visible to assistive apps on macOS.
//!
//! gpui exposes its element tree through AccessKit, which subclasses the
//! window's content view and answers `accessibilityFocusedUIElement` there.
//! AppKit, however, resolves an application's focused element through the
//! key *window*, and the dynamically created window class gpui uses never
//! forwards that question to its content view: every assistive client
//! (VoiceOver, dictation apps) is told the focused element is the window
//! itself. gpui-component works around the same gap for
//! `accessibilityHitTest:`; this does it for the focused element, so a
//! dictation app that asks "is a text field focused?" hears "yes, this one".

#[cfg(target_os = "macos")]
pub fn install_window_focus_forwarder(window: &gpui::Window) {
    use std::ffi::CString;
    use std::ptr::null_mut;

    use objc2::encode::{Encode, EncodeArguments, EncodeReturn, Encoding};
    use objc2::ffi::{class_addMethod, object_getClass};
    use objc2::runtime::{AnyClass, AnyObject, MethodImplementation, Sel};
    use objc2::{msg_send, sel, Message};
    use objc2_app_kit::{NSView, NSWindow};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    extern "C" fn focused_forwarder(this: &NSWindow, _cmd: Sel) -> *mut AnyObject {
        this.contentView().map_or_else(null_mut, |view| unsafe { msg_send![&*view, accessibilityFocusedUIElement] })
    }

    fn ns_view(window: &gpui::Window) -> Option<&NSView> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
        unsafe { (handle.ns_view.as_ptr() as *const NSView).as_ref() }
    }

    unsafe fn add_method<T, F>(class: *mut AnyClass, sel: Sel, func: F)
    where
        T: Message + ?Sized,
        F: MethodImplementation<Callee = T>,
    {
        let encs = F::Arguments::ENCODINGS;
        let mut types = format!("{}{}{}", F::Return::ENCODING_RETURN, <*mut AnyObject>::ENCODING, Sel::ENCODING);
        for enc in encs {
            use std::fmt::Write;
            write!(&mut types, "{enc}").unwrap();
        }
        let types = CString::new(types).unwrap();
        let _ = class_addMethod(class, sel, func.__imp(), types.as_ptr());
    }

    let Some(view) = ns_view(window) else { return };
    let Some(ns_window) = view.window() else { return };
    unsafe {
        let class = object_getClass((&*ns_window as *const NSWindow).cast::<AnyObject>());
        if !class.is_null() {
            add_method(class.cast_mut(), sel!(accessibilityFocusedUIElement), focused_forwarder as extern "C" fn(_, _) -> _);
        }
    }
    let _: Option<&Encoding> = None;
}

#[cfg(not(target_os = "macos"))]
pub fn install_window_focus_forwarder(_window: &gpui::Window) {}
