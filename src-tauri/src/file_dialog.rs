use std::ptr::{null, null_mut};
use windows_sys::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
    OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

pub fn choose_excel_path(
    window: &tauri::WebviewWindow,
    default_name: &str,
) -> Result<Option<String>, String> {
    let mut buffer = vec![0_u16; 32_768];
    let safe_name = default_name
        .chars()
        .map(|ch| {
            if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    let initial: Vec<u16> = safe_name.encode_utf16().collect();
    let copy_len = initial.len().min(buffer.len() - 1);
    buffer[..copy_len].copy_from_slice(&initial[..copy_len]);

    let filter: Vec<u16> = "Excel 工作簿 (*.xlsx)\0*.xlsx\0\0".encode_utf16().collect();
    let title = wide("导出 Excel 字段表");
    let extension = wide("xlsx");
    let owner = window.hwnd().map(|hwnd| hwnd.0 as _).unwrap_or(null_mut());
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        hInstance: null_mut(),
        lpstrFilter: filter.as_ptr(),
        lpstrCustomFilter: null_mut(),
        nMaxCustFilter: 0,
        nFilterIndex: 1,
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        lpstrFileTitle: null_mut(),
        nMaxFileTitle: 0,
        lpstrInitialDir: null(),
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        nFileOffset: 0,
        nFileExtension: 0,
        lpstrDefExt: extension.as_ptr(),
        lCustData: 0,
        lpfnHook: None,
        lpTemplateName: null(),
        pvReserved: null_mut(),
        dwReserved: 0,
        FlagsEx: 0,
    };

    if unsafe { GetSaveFileNameW(&mut dialog) } == 0 {
        let code = unsafe { CommDlgExtendedError() };
        return if code == 0 {
            Ok(None)
        } else {
            Err(format!("无法打开保存窗口，错误代码：0x{code:04X}"))
        };
    }

    let length = buffer.iter().position(|value| *value == 0).unwrap_or(0);
    Ok(Some(String::from_utf16_lossy(&buffer[..length])))
}

pub fn choose_table_path(window: &tauri::WebviewWindow) -> Result<Option<String>, String> {
    let mut buffer = vec![0_u16; 32_768];
    let filter: Vec<u16> = concat!(
        "支持的表格 (*.xlsx;*.xls;*.xlsb;*.ods;*.csv)\0",
        "*.xlsx;*.xls;*.xlsb;*.ods;*.csv\0",
        "Excel 工作簿 (*.xlsx;*.xls;*.xlsb)\0*.xlsx;*.xls;*.xlsb\0",
        "OpenDocument 表格 (*.ods)\0*.ods\0",
        "CSV 文本 (*.csv)\0*.csv\0\0"
    )
    .encode_utf16()
    .collect();
    let title = wide("选择字段表");
    let owner = window.hwnd().map(|hwnd| hwnd.0 as _).unwrap_or(null_mut());
    let mut dialog = base_dialog(owner, &mut buffer, &filter, &title, null());
    dialog.Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST;
    show_dialog(&mut dialog, &buffer, true)
}

pub fn choose_code_path(
    window: &tauri::WebviewWindow,
    default_name: &str,
) -> Result<Option<String>, String> {
    let mut buffer = vec![0_u16; 32_768];
    write_default_name(&mut buffer, default_name);
    let filter: Vec<u16> = "C 头文件 (*.h)\0*.h\0文本文件 (*.txt)\0*.txt\0\0"
        .encode_utf16()
        .collect();
    let title = wide("保存生成的 X-Macro 代码");
    let extension = wide("h");
    let owner = window.hwnd().map(|hwnd| hwnd.0 as _).unwrap_or(null_mut());
    let mut dialog = base_dialog(owner, &mut buffer, &filter, &title, extension.as_ptr());
    dialog.Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
    show_dialog(&mut dialog, &buffer, false)
}

fn base_dialog(
    owner: windows_sys::Win32::Foundation::HWND,
    buffer: &mut [u16],
    filter: &[u16],
    title: &[u16],
    extension: *const u16,
) -> OPENFILENAMEW {
    OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        hInstance: null_mut(),
        lpstrFilter: filter.as_ptr(),
        lpstrCustomFilter: null_mut(),
        nMaxCustFilter: 0,
        nFilterIndex: 1,
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        lpstrFileTitle: null_mut(),
        nMaxFileTitle: 0,
        lpstrInitialDir: null(),
        lpstrTitle: title.as_ptr(),
        Flags: 0,
        nFileOffset: 0,
        nFileExtension: 0,
        lpstrDefExt: extension,
        lCustData: 0,
        lpfnHook: None,
        lpTemplateName: null(),
        pvReserved: null_mut(),
        dwReserved: 0,
        FlagsEx: 0,
    }
}

fn show_dialog(
    dialog: &mut OPENFILENAMEW,
    buffer: &[u16],
    open: bool,
) -> Result<Option<String>, String> {
    let accepted = unsafe {
        if open {
            GetOpenFileNameW(dialog)
        } else {
            GetSaveFileNameW(dialog)
        }
    };
    if accepted == 0 {
        let code = unsafe { CommDlgExtendedError() };
        return if code == 0 {
            Ok(None)
        } else {
            Err(format!("无法打开文件窗口，错误代码：0x{code:04X}"))
        };
    }
    let length = buffer.iter().position(|value| *value == 0).unwrap_or(0);
    Ok(Some(String::from_utf16_lossy(&buffer[..length])))
}

fn write_default_name(buffer: &mut [u16], default_name: &str) {
    let safe_name = default_name
        .chars()
        .map(|ch| {
            if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    let initial: Vec<u16> = safe_name.encode_utf16().collect();
    let copy_len = initial.len().min(buffer.len() - 1);
    buffer[..copy_len].copy_from_slice(&initial[..copy_len]);
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
