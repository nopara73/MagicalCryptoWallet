fn main() {
    println!("cargo:rustc-check-cfg=cfg(mcw_windows_runtime)");
    println!("cargo:rerun-if-env-changed=MCW_WINDOWS_RUNTIME");
    println!("cargo:rerun-if-env-changed=MCW_VERSION");
    if std::env::var("MCW_WINDOWS_RUNTIME").as_deref() != Ok("1") {
        return;
    }
    assert_eq!(std::env::var("TARGET").unwrap(), "x86_64-pc-windows-msvc");
    assert_eq!(std::env::var("CARGO_CFG_PANIC").unwrap(), "abort");
    println!("cargo:rustc-cfg=mcw_windows_runtime");
    for arg in [
        "/ENTRY:mcw_windows_entry",
        "/INCLUDE:_tls_used",
        "/NODEFAULTLIB:msvcrt.lib",
        "/NODEFAULTLIB:msvcrtd.lib",
        "/NODEFAULTLIB:libcmt.lib",
        "/NODEFAULTLIB:libcmtd.lib",
        "/NODEFAULTLIB:vcruntime.lib",
        "/NODEFAULTLIB:vcruntimed.lib",
        "/NODEFAULTLIB:libvcruntime.lib",
        "/NODEFAULTLIB:libvcruntimed.lib",
        "/NODEFAULTLIB:libucrt.lib",
        "/NODEFAULTLIB:libucrtd.lib",
    ] {
        println!("cargo:rustc-link-arg-bin=mcw={arg}");
    }
}
