//! macOS dock / Cmd-Tab icon. Other platforms pick this up from the bundle.

#[cfg(target_os = "macos")]
pub fn apply() {
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSData;
    use objc::{class, msg_send, sel, sel_impl};

    const PNG: &[u8] = include_bytes!("../assets/icons/app.png");

    unsafe {
        let data: id = NSData::dataWithBytes_length_(
            nil,
            PNG.as_ptr().cast(),
            PNG.len() as cocoa::foundation::NSUInteger,
        );
        if data == nil {
            return;
        }
        let image: id = msg_send![class!(NSImage), alloc];
        let image: id = msg_send![image, initWithData: data];
        if image == nil {
            return;
        }
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply() {}
