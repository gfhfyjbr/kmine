pub fn platform_id(os: &str, arch: &str) -> String {
    match (os, arch) {
        ("linux", "x86_64") => "linux".into(),
        ("linux", "x86") => "linux-i386".into(),
        ("macos", "x86_64") => "mac-os".into(),
        ("macos", "aarch64") => "mac-os-arm64".into(),
        ("windows", "x86_64") => "windows-x64".into(),
        ("windows", "aarch64") => "windows-arm64".into(),
        ("windows", "x86") => "windows-x86".into(),
        _ => format!("{os}-{arch}"),
    }
}
