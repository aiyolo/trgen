use std::{
    ptr::{null, null_mut},
    sync::{mpsc, OnceLock},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW,
        GetStockObject, InvalidateRect, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor,
        UpdateWindow, DEFAULT_GUI_FONT, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBRUSH, MONITORINFO,
        MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, TRANSPARENT,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetCursorPos,
        GetMessageW, GetSystemMetrics, KillTimer, PostMessageW, RegisterClassW, SetTimer,
        ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, MSG, SM_CXSCREEN, SM_CYSCREEN,
        SW_SHOW, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    },
};

const WIDTH: i32 = 420;
const HEIGHT: i32 = 238;
const TIMER_ID: usize = 1;
static PROGRESS: OnceLock<std::sync::atomic::AtomicU32> = OnceLock::new();

pub struct NativeSplash {
    hwnd: isize,
}

impl NativeSplash {
    pub fn show() -> Option<Self> {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || unsafe { run_splash(sender) });
        receiver
            .recv_timeout(Duration::from_millis(250))
            .ok()
            .filter(|hwnd| *hwnd != 0)
            .map(|hwnd| Self { hwnd })
    }

    pub fn close(self) {
        if self.hwnd != 0 {
            unsafe {
                PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
            }
        }
    }
}

unsafe fn run_splash(sender: mpsc::SyncSender<isize>) {
    let class_name = wide("StructSheetNativeSplash");
    let title = wide("StructSheet");
    let instance = unsafe { GetModuleHandleW(null()) };
    let background = unsafe { CreateSolidBrush(rgb(244, 242, 236)) };
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hbrBackground: background,
        lpszClassName: class_name.as_ptr(),
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { RegisterClassW(&class) };

    let (x, y) = splash_position();
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            WIDTH,
            HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    let _ = sender.send(hwnd as isize);
    if hwnd.is_null() {
        unsafe { DeleteObject(background) };
        return;
    }

    PROGRESS.get_or_init(|| std::sync::atomic::AtomicU32::new(12));
    unsafe {
        SetTimer(hwnd, TIMER_ID, 90, None);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }

    let mut message: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe { DeleteObject(background) };
}

fn splash_position() -> (i32, i32) {
    unsafe {
        let mut cursor = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut cursor) != 0 {
            let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
            if !monitor.is_null() {
                let mut info: MONITORINFO = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(monitor, &mut info) != 0 {
                    return center_in_rect(info.rcMonitor, WIDTH, HEIGHT);
                }
            }
        }

        center_in_rect(
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            },
            WIDTH,
            HEIGHT,
        )
    }
}

fn center_in_rect(area: RECT, width: i32, height: i32) -> (i32, i32) {
    (
        area.left + ((area.right - area.left - width).max(0) / 2),
        area.top + ((area.bottom - area.top - height).max(0) / 2),
    )
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER => {
            if wparam == TIMER_ID {
                if let Some(progress) = PROGRESS.get() {
                    let current = progress.load(std::sync::atomic::Ordering::Relaxed);
                    let step = ((90_u32.saturating_sub(current)) / 7).max(1);
                    progress.store(
                        (current + step).min(90),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                unsafe { InvalidateRect(hwnd, null(), 0) };
            }
            0
        }
        WM_PAINT => {
            unsafe { paint(hwnd) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_CLOSE => {
            unsafe {
                KillTimer(hwnd, TIMER_ID);
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
    let dc = unsafe { BeginPaint(hwnd, &mut paint) };
    let mut client: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };

    fill(dc, &client, rgb(244, 242, 236));
    unsafe {
        SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, rgb(40, 39, 31));
    }

    let mut brand_rect = RECT {
        left: 176,
        top: 35,
        right: 244,
        bottom: 92,
    };
    fill(dc, &brand_rect, rgb(244, 227, 34));
    draw_text(
        dc,
        "{ }",
        &mut brand_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    let mut title_rect = RECT {
        left: 20,
        top: 105,
        right: 400,
        bottom: 135,
    };
    draw_text(
        dc,
        "StructSheet",
        &mut title_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    let track = RECT {
        left: 90,
        top: 158,
        right: 330,
        bottom: 164,
    };
    fill(dc, &track, rgb(222, 219, 210));
    let progress = PROGRESS
        .get()
        .map(|value| value.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(12);
    let bar = RECT {
        left: track.left,
        top: track.top,
        right: track.left + ((track.right - track.left) * progress as i32 / 100),
        bottom: track.bottom,
    };
    fill(dc, &bar, rgb(40, 39, 31));

    unsafe { SetTextColor(dc, rgb(126, 123, 114)) };
    let mut status_rect = RECT {
        left: 20,
        top: 173,
        right: 400,
        bottom: 198,
    };
    draw_text(
        dc,
        &format!("正在启动… {progress}%"),
        &mut status_rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    unsafe { EndPaint(hwnd, &paint) };
}

fn fill(dc: *mut core::ffi::c_void, rect: &RECT, color: u32) {
    unsafe {
        let brush: HBRUSH = CreateSolidBrush(color);
        FillRect(dc, rect, brush);
        DeleteObject(brush);
    }
}

fn draw_text(dc: *mut core::ffi::c_void, text: &str, rect: &mut RECT, format: u32) {
    let text = wide(text);
    unsafe {
        DrawTextW(dc, text.as_ptr(), (text.len() - 1) as i32, rect, format);
    }
}

fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_inside_non_primary_monitor() {
        let area = RECT {
            left: 1920,
            top: 0,
            right: 4480,
            bottom: 1440,
        };
        assert_eq!(center_in_rect(area, 420, 238), (2990, 601));
    }
}
