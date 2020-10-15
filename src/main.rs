
use winapi::um::winuser::*;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::shared::windef::{HWND};
use winapi::shared::minwindef::{UINT, WPARAM, LPARAM, LRESULT};
use bedrock as br;

fn main() {
    let wce = WNDCLASSEXA {
        cbSize: std::mem::size_of::<WNDCLASSEXA>() as _,
        lpszClassName: b"jp.ct2.experimental.vkNoRedirectRender\0".as_ptr() as _,
        lpfnWndProc: Some(wcb),
        hInstance: unsafe { GetModuleHandleA(std::ptr::null_mut()) },
        .. unsafe { std::mem::MaybeUninit::zeroed().assume_init() }
    };
    if unsafe { RegisterClassExA(&wce) == 0 } {
        panic!("RegisterClassEx failed: {:?}", std::io::Error::last_os_error());
    }

    let w = unsafe {
        CreateWindowExA(
            WS_EX_APPWINDOW | WS_EX_OVERLAPPEDWINDOW | WS_EX_NOREDIRECTIONBITMAP,
            wce.lpszClassName, b"vkNoRedirectRender\0".as_ptr() as _,
            WS_OVERLAPPEDWINDOW | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
            std::ptr::null_mut(), std::ptr::null_mut(), wce.hInstance, std::ptr::null_mut()
        )
    };
    if w.is_null() {
        panic!("CreateWindowEx failed: {:?}", std::io::Error::last_os_error());
    }

    let mut msg = unsafe { std::mem::MaybeUninit::uninit().assume_init() };
    while unsafe { GetMessageA(&mut msg, std::ptr::null_mut(), 0, 0) > 0 } {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
    }
}

extern "system" fn wcb(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => unsafe { PostQuitMessage(0); return 0; },
        _ => ()
    }

    unsafe { DefWindowProcA(hwnd, msg, wp, lp) }
}
