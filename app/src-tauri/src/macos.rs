//! macOS pieces of the shell: the clipboard (NSPasteboard) and the two native alerts the tray needs
//! (CFUserNotification: blocking, callable from any thread).

pub mod clipboard {
    use objc2_app_kit::{NSPasteboard, NSPasteboardContentsOptions, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    use tauri::{AppHandle, Runtime};

    /// Copy `text` to the general pasteboard. A secret stays on this Mac (no Universal Clipboard) and carries the
    /// nspasteboard.org markers that ask clipboard managers not to show (`ConcealedType`) or record
    /// (`TransientType`) it: the macOS counterparts of the Windows formats.
    pub fn copy<R: Runtime>(_app: &AppHandle<R>, text: &str, secret: bool) -> Result<(), String> {
        let pb = NSPasteboard::generalPasteboard();
        if secret {
            pb.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
        } else {
            pb.clearContents();
        }
        let ok = pb.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
        if !ok {
            return Err("The clipboard did not take the text. Try again.".into());
        }
        if secret {
            let empty = NSString::from_str("");
            for marker in ["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"] {
                pb.setString_forType(&empty, &NSString::from_str(marker));
            }
        }
        Ok(())
    }
}

pub mod alert {
    use core_foundation_sys::base::{kCFAllocatorDefault, CFOptionFlags, CFRelease};
    use core_foundation_sys::string::{kCFStringEncodingUTF8, CFStringCreateWithBytes, CFStringRef};
    use core_foundation_sys::user_notification::{
        kCFUserNotificationAlternateResponse, kCFUserNotificationCautionAlertLevel, kCFUserNotificationNoteAlertLevel,
        CFUserNotificationDisplayAlert,
    };

    /// A CFString this code owns.
    struct Text(CFStringRef);

    impl Text {
        fn new(s: &str) -> Text {
            Text(unsafe { CFStringCreateWithBytes(kCFAllocatorDefault, s.as_ptr(), s.len() as _, kCFStringEncodingUTF8, 0) })
        }
    }

    impl Drop for Text {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0.cast()) }
            }
        }
    }

    /// Show an alert and wait for it; `alternate` adds a second button. Returns the response flags.
    fn show(level: CFOptionFlags, title: &str, text: &str, default: &str, alternate: Option<&str>) -> Option<CFOptionFlags> {
        let (t, m, d) = (Text::new(title), Text::new(text), Text::new(default));
        let a = alternate.map(Text::new);
        let mut response: CFOptionFlags = 0;
        let r = unsafe {
            CFUserNotificationDisplayAlert(
                0.0,
                level,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                t.0,
                m.0,
                d.0,
                a.as_ref().map_or(std::ptr::null(), |a| a.0),
                std::ptr::null(),
                &mut response,
            )
        };
        (r == 0).then_some(response & 0x3)
    }

    /// Yes / No; the default button (Return) is No, as on Windows.
    pub fn confirm(title: &str, text: &str) -> bool {
        show(kCFUserNotificationCautionAlertLevel, title, text, "No", Some("Yes")) == Some(kCFUserNotificationAlternateResponse)
    }

    pub fn message(title: &str, text: &str) {
        let _ = show(kCFUserNotificationNoteAlertLevel, title, text, "OK", None);
    }
}
