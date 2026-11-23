//! Keep the standard Windows menu rendering instead of nsh's dark GDI menu bar.
//! All calls run on the owning UI thread; no registry or global hooks are used.
use windows_sys::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Threading::GetCurrentThreadId,
        },
        UI::{
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                DefWindowProcW, DrawMenuBar, EnumThreadWindows, GetMenu, WM_INITMENUPOPUP,
                WM_NCACTIVATE, WM_NCDESTROY, WM_NCPAINT,
            },
        },
    },
    core::{BOOL, w},
};

const SUBCLASS_ID: usize = 0x53334d4e;
const WM_UAHDRAWMENU: u32 = 0x0091;
const WM_UAHDRAWMENUITEM: u32 = 0x0092;

pub fn use_system_menus() {
    // SAFETY: scheduled by run_on_main_thread after the menu mount commands.
    // EnumThreadWindows visits only windows owned by this UI thread.
    unsafe {
        system_menu_theme();
        EnumThreadWindows(GetCurrentThreadId(), Some(install), 0);
    }
}

unsafe extern "system" fn install(hwnd: HWND, _: LPARAM) -> BOOL {
    // SAFETY: hwnd belongs to this thread, and the callback has static lifetime.
    unsafe {
        if !GetMenu(hwnd).is_null() {
            if SetWindowSubclass(hwnd, Some(menu_proc), SUBCLASS_ID, 0) != 0 {
                DrawMenuBar(hwnd);
            }
        }
    }
    1
}

fn system_paints(message: u32) -> bool {
    matches!(
        message,
        WM_UAHDRAWMENU | WM_UAHDRAWMENUITEM | WM_NCPAINT | WM_NCACTIVATE
    )
}

unsafe extern "system" fn menu_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // SAFETY: Windows invokes this on the window's owning thread. Only menu/frame
    // painting is bypassed; commands, keyboard navigation and all other events
    // continue through the framework's existing window procedure.
    unsafe {
        if message == WM_NCDESTROY {
            RemoveWindowSubclass(hwnd, Some(menu_proc), SUBCLASS_ID);
        } else if !GetMenu(hwnd).is_null() {
            if message == WM_INITMENUPOPUP {
                system_menu_theme();
            }
            if system_paints(message) {
                system_menu_theme();
                if matches!(message, WM_NCPAINT | WM_NCACTIVATE) {
                    // Preserve nsh's activation/invalidation bookkeeping, then
                    // let Windows repaint the frame last (without the GDI line).
                    let _ = DefSubclassProc(hwnd, message, wparam, lparam);
                }
                return DefWindowProcW(hwnd, message, wparam, lparam);
            }
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

unsafe fn system_menu_theme() {
    // Undo nsh's process-local ForceDark/ForceLight preference. These are the
    // same uxtheme entry points used by nsh v0.6.1; use Default (0), never a
    // registry setting. Missing entry points on older Windows are a no-op.
    type SetMode = unsafe extern "system" fn(i32) -> i32;
    type Flush = unsafe extern "system" fn();
    unsafe {
        let module = GetModuleHandleW(w!("uxtheme.dll"));
        if module.is_null() {
            return;
        }
        let (Some(set), Some(flush)) = (
            GetProcAddress(module, 135usize as *const u8),
            GetProcAddress(module, 136usize as *const u8),
        ) else {
            return;
        };
        let set: SetMode = std::mem::transmute(set);
        let flush: Flush = std::mem::transmute(flush);
        if set(0) != 0 {
            flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::{
        Shell::GetWindowSubclass,
        WindowsAndMessaging::{
            AppendMenuW, CreateMenu, CreateWindowExW, DestroyWindow, MF_STRING, SetMenu,
            WM_COMMAND, WM_KEYDOWN, WS_OVERLAPPEDWINDOW,
        },
    };

    #[test]
    fn system_renderer_does_not_intercept_menu_commands_or_keys() {
        assert!(system_paints(WM_UAHDRAWMENU));
        assert!(system_paints(WM_NCPAINT));
        assert!(!system_paints(WM_COMMAND));
        assert!(!system_paints(WM_KEYDOWN));
    }

    #[test]
    fn installs_on_native_menu_windows_without_replacing_the_menu() {
        // SAFETY: a hidden standard window and its menu, created and destroyed
        // on this test thread. DestroyWindow also destroys its attached menu.
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                w!("STATIC"),
                w!("S3 Studio menu test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                200,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());
            let menu = CreateMenu();
            assert!(!menu.is_null());
            assert_ne!(AppendMenuW(menu, MF_STRING, 1, w!("S3 Studio")), 0);
            assert_ne!(SetMenu(hwnd, menu), 0);
            install(hwnd, 0);
            let mut data = 0;
            assert_ne!(
                GetWindowSubclass(hwnd, Some(menu_proc), SUBCLASS_ID, &mut data),
                0
            );
            assert_eq!(GetMenu(hwnd), menu);
            assert_ne!(DestroyWindow(hwnd), 0);
        }
    }
}
