//! Fallback dock / Cmd-Tab icon when the process is not inside kmine.app.
//!
//! The real icon is the Icon Composer `AppIcon.icon`, compiled to
//! `Assets.car` inside the bundle by `scripts/macos-run.sh`. `.icon` cannot
//! be loaded as an `NSImage` (`NSImage(contentsOf:)` returns nil), so this
//! fallback is a square raster — full-bleed fill + petal, no baked squircle.
//! A pre-rounded 1024px bezel aliases when Dock scales it down.

pub fn apply() {
    #[cfg(target_os = "macos")]
    apply_macos();
}

#[cfg(target_os = "macos")]
fn apply_macos() {
    if running_from_app_bundle() {
        return;
    }

    use cocoa::base::{id, nil};
    use cocoa::foundation::NSData;
    use objc::{class, msg_send, sel, sel_impl};

    const PNG: &[u8] = include_bytes!("../assets/icon/AppIcon-square.png");

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

#[cfg(target_os = "macos")]
fn running_from_app_bundle() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let macos = match exe.parent() {
        Some(path) => path,
        None => return false,
    };
    let contents = match macos.parent() {
        Some(path) => path,
        None => return false,
    };
    let app = match contents.parent() {
        Some(path) => path,
        None => return false,
    };
    macos.file_name().is_some_and(|name| name == "MacOS")
        && contents.file_name().is_some_and(|name| name == "Contents")
        && app.extension().is_some_and(|ext| ext == "app")
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn bundle_layout_matches_macos_convention() {
        use std::path::PathBuf;
        let exe = PathBuf::from("/tmp/kmine.app/Contents/MacOS/kmine");
        let macos = exe.parent().unwrap();
        let contents = macos.parent().unwrap();
        let app = contents.parent().unwrap();
        assert_eq!(macos.file_name().unwrap(), "MacOS");
        assert_eq!(contents.file_name().unwrap(), "Contents");
        assert_eq!(app.extension().unwrap(), "app");
    }
}
