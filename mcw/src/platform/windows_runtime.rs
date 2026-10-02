//! First-party Windows x64 runtime primitives, PE TLS description and entry point.
//!
//! The supported application build rebuilds Rust std with panic=abort and links
//! only OS import libraries. No C/C++ exception personality is substituted.
//! The functions below must never allocate, panic, or compile into calls to
//! themselves. Inline assembly and volatile byte accesses enforce that property.
use core::ffi::c_void;

#[cfg(target_feature = "crt-static")]
compile_error!("Magical Crypto Wallet must not statically link a redistributable CRT");
#[cfg(all(mcw_windows_runtime, not(panic = "abort")))]
compile_error!(
    "The first-party Windows runtime requires an aborting standard library and application"
);

/// # Safety
/// For nonzero length, source and destination must be valid nonoverlapping byte
/// ranges. No alignment is required. Zero length accepts null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(
    destination: *mut c_void,
    source: *const c_void,
    length: usize,
) -> *mut c_void {
    // SAFETY: the caller supplies valid nonoverlapping ranges. REP MOVSB accesses
    // exactly RCX bytes, does not dereference either pointer for RCX=0, and cannot
    // be folded by LLVM into another memcpy. Rust preserves Win64's RSI/RDI.
    unsafe {
        core::arch::asm!(
            "rep movsb",
            inout("rdi") destination => _,
            inout("rsi") source => _,
            inout("rcx") length => _,
            options(nostack, preserves_flags),
        );
    }
    destination
}

/// # Safety
/// For nonzero length, both pointers must denote valid byte ranges of length.
/// Overlap and any alignment are supported. Zero length accepts null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(
    destination: *mut c_void,
    source: *const c_void,
    length: usize,
) -> *mut c_void {
    let target = destination.cast::<u8>();
    let input = source.cast::<u8>();
    if target.addr() > input.addr() && target.addr() - input.addr() < length {
        // Copy backwards without setting the CPU direction flag. In particular,
        // Windows exception dispatch must never observe a temporarily set DF.
        let mut remaining = length;
        while remaining != 0 {
            remaining -= 1;
            // SAFETY: remaining is inside both caller-provided ranges; reading
            // before writing preserves each original byte with backwards overlap.
            // Volatile byte operations prevent LLVM's memmove/memcpy loop folding.
            unsafe {
                target
                    .add(remaining)
                    .write_volatile(input.add(remaining).read_volatile());
            }
        }
    } else {
        // SAFETY: bytewise forward copying also preserves overlapping ranges when
        // destination <= source. The assembly does not assert noalias, unlike a
        // Rust copy_nonoverlapping intrinsic or a call to the memcpy symbol.
        unsafe {
            core::arch::asm!(
                "rep movsb",
                inout("rdi") destination => _,
                inout("rsi") source => _,
                inout("rcx") length => _,
                options(nostack, preserves_flags),
            );
        }
    }
    destination
}

/// # Safety
/// destination must be writable for length bytes. Alignment is unrestricted;
/// zero length accepts null. The low eight bits of value are written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(
    destination: *mut c_void,
    value: i32,
    length: usize,
) -> *mut c_void {
    // SAFETY: caller supplies a writable range. REP STOSB touches exactly length
    // bytes and performs no access for zero; it cannot recursively call memset.
    unsafe {
        core::arch::asm!(
            "rep stosb",
            inout("rdi") destination => _,
            inout("rcx") length => _,
            in("al") value as u8,
            options(nostack, preserves_flags),
        );
    }
    destination
}

/// # Safety
/// Both pointers must be readable for length bytes, with any alignment. Zero
/// length accepts null. This is lexicographic comparison, not constant-time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(left: *const c_void, right: *const c_void, length: usize) -> i32 {
    let left = left.cast::<u8>();
    let right = right.cast::<u8>();
    let mut offset = 0;
    while offset < length {
        // SAFETY: offset lies in both caller-provided ranges. Volatile reads
        // prohibit LLVM from recognizing and replacing this loop with memcmp.
        let (a, b) = unsafe {
            (
                left.add(offset).read_volatile(),
                right.add(offset).read_volatile(),
            )
        };
        if a != b {
            return i32::from(a) - i32::from(b);
        }
        offset += 1;
    }
    0
}

#[cfg(mcw_windows_runtime)]
mod startup {
    use super::run_exit_handlers;
    // IMAGE_TLS_DIRECTORY64 describes TLS to the Windows loader. Rust std's native
    // thread locals are initialized by that loader and its FLS callbacks run their
    // destructors. This is PE metadata, not a copied CRT initialization routine.
    #[repr(C)]
    struct TlsDirectory {
        start: *const u8,
        end: *const u8,
        index: *mut u32,
        callbacks: *const usize,
        zero_fill: u32,
        characteristics: u32,
    }
    // SAFETY: the directory contains only process-lifetime addresses. The OS writes
    // the index before Rust runs; Rust never accesses it through a shared reference.
    unsafe impl Sync for TlsDirectory {}

    #[repr(C, align(64))]
    struct TlsStart([u8; 64]);
    #[used]
    #[unsafe(link_section = ".tls")]
    static mut TLS_START: TlsStart = TlsStart([0; 64]);
    #[used]
    #[unsafe(link_section = ".tls$ZZZ")]
    static mut TLS_END: u8 = 0;
    #[unsafe(no_mangle)]
    static mut _tls_index: u32 = 0;
    static TLS_CALLBACKS: [usize; 1] = [0];
    #[used]
    #[unsafe(no_mangle)]
    static _tls_used: TlsDirectory = TlsDirectory {
        start: (&raw const TLS_START).cast(),
        end: &raw const TLS_END,
        index: &raw mut _tls_index,
        callbacks: TLS_CALLBACKS.as_ptr(),
        zero_fill: 0,
        characteristics: 7 << 20, // IMAGE_SCN_ALIGN_64BYTES, matching TLS_START.
    };

    unsafe extern "C" {
        // rustc emits this C ABI wrapper for a conventional Rust binary. It calls
        // lang_start, preserving Rust's stack-overflow handler, main thread setup,
        // standard I/O cleanup and main's ordinary return handling. On Windows std
        // ignores argc/argv and obtains its arguments from GetCommandLineW.
        #[link_name = "main"]
        fn rust_main(argc: i32, argv: *const *const u8) -> i32;
    }
    #[link(name = "kernel32", kind = "raw-dylib")]
    unsafe extern "system" {
        fn ExitProcess(code: u32) -> !;
    }
    #[link(name = "kernelbase", kind = "raw-dylib")]
    unsafe extern "system" {
        // This documented Windows stack-probing routine uses the compiler's special
        // RAX convention. Never call it as a Rust function: the import thunk only
        // jumps to the OS implementation for compiler-generated calls, preserving
        // registers and the stack/guard-page security checks. Windows 10+ provides it.
        fn __chkstk();
    }

    /// Entry point selected only by Contrib/Mcw/build-windows.ps1.
    /// # Safety
    /// Must be invoked once by the Windows loader after PE/TLS initialization.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn mcw_windows_entry() -> ! {
        // SAFETY: Windows std ignores these arguments; the compiler wrapper performs
        // Rust runtime initialization once. No Microsoft CRT startup is linked.
        let code = unsafe { rust_main(0, core::ptr::null()) };
        run_exit_handlers();
        // SAFETY: main has returned and lang_start has completed its normal cleanup.
        unsafe { ExitProcess(code as u32) }
    }
}

type ExitHandler = unsafe extern "C" fn();
struct ExitRegistry {
    handlers: Vec<ExitHandler>,
    finished: bool,
}
static EXIT_HANDLERS: std::sync::Mutex<ExitRegistry> = std::sync::Mutex::new(ExitRegistry {
    handlers: Vec::new(),
    finished: false,
});

/// Register a process-exit callback in reverse registration order.
/// # Safety
/// callback must remain executable for the process lifetime and may not unwind
/// across its C ABI. This executable-only registry does not support DLL unload.
#[cfg_attr(mcw_windows_runtime, unsafe(no_mangle))]
pub unsafe extern "C" fn atexit(callback: ExitHandler) -> i32 {
    let Ok(mut registry) = EXIT_HANDLERS.lock() else {
        return 1;
    };
    if registry.finished || registry.handlers.try_reserve(1).is_err() {
        return 1;
    }
    registry.handlers.push(callback);
    0
}

#[cfg(mcw_windows_runtime)]
fn run_exit_handlers() {
    loop {
        let callback = {
            let mut registry = EXIT_HANDLERS.lock().unwrap_or_else(|e| e.into_inner());
            let callback = registry.handlers.pop();
            if callback.is_none() {
                registry.finished = true;
            }
            callback
        };
        let Some(callback) = callback else { break };
        // SAFETY: atexit's caller guarantees a process-lifetime nonunwinding
        // callback. Release the lock before calling it so new registrations can
        // be processed too, and so callback code may safely take other locks.
        unsafe { callback() };
    }
}
