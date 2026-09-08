//! Best-effort Windows unhandled-exception breadcrumb. No allocation, locks, stack
//! walking, file creation or UI from the faulting thread. Supervisor owns reporting.
use std::{ffi::c_void, sync::atomic::{AtomicUsize, Ordering}};
static IMAGE_BASE: AtomicUsize = AtomicUsize::new(0);
static STDERR: AtomicUsize = AtomicUsize::new(0);

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    nested: *const ExceptionRecord,
    address: *const c_void,
    count: u32,
    information: [usize; 15],
}
#[repr(C)]
struct ExceptionPointers {
    record: *const ExceptionRecord,
    context: *const c_void,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetUnhandledExceptionFilter(filter: unsafe extern "system" fn(*const ExceptionPointers) -> i32) -> *const c_void;
    fn GetModuleHandleW(name: *const u16) -> *const c_void;
    fn GetStdHandle(kind: u32) -> *mut c_void;
    fn WriteFile(file: *mut c_void, buffer: *const u8, size: u32, written: *mut u32, overlapped: *mut c_void) -> i32;
}
pub(super) fn install() {
    // SAFETY: null requests this executable; -12 requests stderr. Handles are
    // borrowed for the process lifetime, and our filter has the Windows ABI.
    unsafe {
        IMAGE_BASE.store(GetModuleHandleW(std::ptr::null()) as usize, Ordering::Relaxed);
        STDERR.store(GetStdHandle((-12i32) as u32) as usize, Ordering::Relaxed);
        SetUnhandledExceptionFilter(filter);
    }
}
unsafe extern "system" fn filter(pointers: *const ExceptionPointers) -> i32 {
    if pointers.is_null() { return 0; }
    // SAFETY: Windows supplies a live EXCEPTION_POINTERS and EXCEPTION_RECORD
    // for the duration of the top-level filter. Do not dereference the context.
    let record = unsafe { (*pointers).record };
    if record.is_null() { return 0; }
    let record = unsafe { &*record };
    let mut output = [0u8; 256];
    let mut used = 0;
    fn append(output: &mut [u8], used: &mut usize, value: &[u8]) {
        output[*used..*used + value.len()].copy_from_slice(value); *used += value.len();
    }
    fn hex(output: &mut [u8], used: &mut usize, value: usize) {
        for shift in (0..std::mem::size_of::<usize>() * 2).rev() {
            output[*used] = b"0123456789ABCDEF"[(value >> (shift * 4)) & 15]; *used += 1;
        }
    }
    append(&mut output, &mut used, b"\nREPORT_NATIVE code=0x");
    hex(&mut output, &mut used, record.code as usize);
    append(&mut output, &mut used, b" fault_pc=0x");
    hex(&mut output, &mut used, record.address as usize);
    append(&mut output, &mut used, b" executable_image_base=0x");
    hex(&mut output, &mut used, IMAGE_BASE.load(Ordering::Relaxed));
    append(&mut output, &mut used, b" (address may belong to another module)\n");
    let mut written = 0;
    // SAFETY: bounded stack buffer stays live through synchronous WriteFile.
    // Failure is intentionally ignored; Windows continues its exception search.
    unsafe { WriteFile(STDERR.load(Ordering::Relaxed) as *mut c_void, output.as_ptr(), used as u32, &mut written, std::ptr::null_mut()); }
    0
}
