#![allow(unused_imports)]
use core::slice;
use std::error::Error;
use std::f32::consts::PI;
use std::ptr::null_mut;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{thread, u8};

use opencv::types::VectorOfu8;
use windows::core::Interface;
use windows::core::HSTRING;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat};
use windows::Storage::{CreationCollisionOption, FileAccessMode, StorageFolder};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{self, XFORM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VK_LBUTTON, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    self, CallNextHookEx, MSG, WINDOWS_HOOK_ID, WM_LBUTTONDOWN, WM_LBUTTONUP,
};

use opencv::core::{Mat, Point2f, BORDER_CONSTANT};
use opencv::imgproc;
use opencv::prelude::*;

mod capture;
mod window_info;

#[derive(Debug)]
struct Hit {
    indicator: u32,
    range: [u32; 2],
}

fn main() {
    let hwnd = unsafe { WindowsAndMessaging::GetDesktopWindow() };
    let mut hit = Hit {
        indicator: 0,
        range: [0, 0],
    };

    // 保留以下代码用来优化
    //
    // let mouse_hook = unsafe {
    //     WindowsAndMessaging::SetWindowsHookExA(WINDOWS_HOOK_ID(14), Some(mouse_hook_proc), None, 0)
    // };

    // // 消息循环
    // let mut msg = MSG::default();
    // while unsafe { WindowsAndMessaging::GetMessageA(&mut msg, None, 0, 0) }.0 > 0 {
    //     unsafe {
    //         WindowsAndMessaging::TranslateMessage(&msg);
    //         WindowsAndMessaging::DispatchMessageA(&msg);
    //     }
    // }

    // unsafe {
    //     let _ = match mouse_hook {
    //         Ok(hook) => {
    //             WindowsAndMessaging::UnhookWindowsHookEx(hook);
    //         }
    //         Err(_) => (),
    //     };
    // }

    let handle = thread::spawn(move || loop {
        if unsafe { GetAsyncKeyState(1) } != 0 {
            screenshot_by_hwnd(hwnd, &mut hit).unwrap();
        }
    });

    handle.join().unwrap();
}

fn screenshot_by_hwnd(hwnd: HWND, hit_share: &mut Hit) -> Result<(), Box<dyn Error>> {
    Ok(unsafe {
        let hdc = Gdi::GetWindowDC(hwnd);
        let mut rect = RECT::default();
        WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
        let radius = 87;
        let diameter = radius * 2;
        let capture_x = (rect.right - rect.left) / 2 - radius;
        // 任务栏高度是 40 所以减 20
        let taskbar_height = 20;
        let capture_y = (rect.bottom - rect.top) / 2 - radius - taskbar_height;

        let hdc_dest = Gdi::CreateCompatibleDC(hdc);
        let h_bitmap = Gdi::CreateCompatibleBitmap(hdc, diameter, diameter);
        let h_old = Gdi::SelectObject(hdc_dest, h_bitmap);

        // 目标, .... 源
        Gdi::BitBlt(
            hdc_dest,
            0,
            0,
            diameter,
            diameter,
            hdc,
            capture_x,
            capture_y,
            Gdi::SRCCOPY,
        );

        let mut buffer: Vec<u8> = vec![0; (diameter * diameter * 4) as usize];

        let bitmap_info_header = Gdi::BITMAPINFOHEADER {
            biSize: std::mem::size_of::<Gdi::BITMAPINFOHEADER>() as u32,
            biWidth: diameter,
            biHeight: -diameter,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };

        let mut bitmap_info = Gdi::BITMAPINFO {
            bmiHeader: bitmap_info_header,
            bmiColors: [Gdi::RGBQUAD::default(); 1],
        };

        Gdi::GetDIBits(
            hdc_dest,
            h_bitmap,
            0,
            diameter as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bitmap_info,
            Gdi::DIB_RGB_COLORS,
        );

        Gdi::SelectObject(hdc_dest, h_old);
        Gdi::DeleteDC(hdc_dest);
        Gdi::ReleaseDC(hwnd, hdc);
        Gdi::DeleteObject(h_bitmap);

        let width = diameter;
        let height = diameter;
        let mut rotated_buffer: Vec<u8> = vec![0; (width * height * 4) as usize];

        let now = std::time::SystemTime::now();

        for angle in 1..360 {
            // 优化执行耗时
            if angle % 2 == 0 {
                continue;
            }
            rotated_buffer = rotate_img_buffer(&mut buffer, angle as f64)?;

            let mut count = 0;
            let mut rows_iter = (1..11).into_iter();
            let mut row_num = rows_iter.next();

            for pixel in rotated_buffer.chunks(4) {
                if row_num.is_none() {
                    break;
                }
                if count == radius * row_num.unwrap() {
                    row_num = rows_iter.next();
                    if pixel[3] == 0 || pixel[3] != 255 {
                        continue;
                    }
                    // RGBA -> 2103
                    if pixel[0] < 50 && pixel[1] < 50 && pixel[2] > 200 {
                        hit_share.indicator = angle;
                    }
                    if pixel[0] > 240 && pixel[1] > 240 && pixel[2] > 240 {
                        if hit_share.range == [0, 0] {
                            hit_share.range = [angle, angle];
                        } else if angle < hit_share.range[0] {
                            hit_share.range[0] = angle;
                        } else if angle > hit_share.range[1] {
                            hit_share.range[1] = angle;
                        }
                    }
                }
                count += 1;
            }
        }

        {
            // 在此进行触发 SPACE
            let indicator_degree = hit_share.indicator;
            let white_region = hit_share.range;
            if white_region[0] != 0
                && indicator_degree > white_region[0]
                && indicator_degree < white_region[1]
            {
                let _ = press_space();
                println!(
                    "红色命中: {} / 白色区域范围 [{}, {}]",
                    indicator_degree, white_region[0], white_region[1]
                );

                hit_share.range = [0, 0];
                hit_share.indicator = 0;
                // save_buffer_to_image(diameter as u32, diameter as u32, &rotated_buffer)?;
            }
        }

        let elapsed = now.elapsed().unwrap();
        println!("内部循环耗时: {}ms", elapsed.as_millis());
    })
}

fn save_buffer_to_image(width: u32, height: u32, buffer: &Vec<u8>) -> Result<(), Box<dyn Error>> {
    let path = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(&path))?.get()?;
    let file = folder
        .CreateFileAsync(
            &HSTRING::from("screenshot.png"),
            CreationCollisionOption::ReplaceExisting,
        )?
        .get()?;

    let stream = file.OpenAsync(FileAccessMode::ReadWrite)?.get()?;
    let encoder = BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId()?, &stream)?.get()?;
    encoder.SetPixelData(
        BitmapPixelFormat::Bgra8,
        // BitmapPixelFormat::Rgba8,
        BitmapAlphaMode::Premultiplied,
        width,
        height,
        1.0,
        1.0,
        buffer,
    )?;

    encoder.FlushAsync()?.get()?;

    Ok(())
}

fn rotate_img_buffer(buffer: &mut Vec<u8>, angle: f64) -> opencv::Result<Vec<u8>, Box<dyn Error>> {
    let img = unsafe {
        Mat::new_rows_cols_with_data(
            174,
            174,
            opencv::core::CV_8UC4,
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            opencv::core::Mat_AUTO_STEP,
        )?
    };

    let center = Point2f::new(img.cols() as f32 / 2.0, img.rows() as f32 / 2.0);

    let rot_matrix = imgproc::get_rotation_matrix_2d(center, angle, 1.0)?;

    let mut rotated_img = img.clone();

    imgproc::warp_affine(
        &img,
        &mut rotated_img,
        &rot_matrix,
        img.size()?,
        imgproc::INTER_LINEAR,
        opencv::core::BORDER_CONSTANT,
        Default::default(),
    )?;
    let img_data_ptr = rotated_img.data();

    let img_data_size =
        (rotated_img.rows() * rotated_img.cols() * rotated_img.elem_size()? as i32) as usize;

    let img_vec: Vec<u8> = unsafe { slice::from_raw_parts(img_data_ptr, img_data_size).to_vec() };
    Ok(img_vec)
}

fn press_space() -> windows::core::Result<()> {
    let mut input_down: INPUT = INPUT::default();
    let key_input = KEYBDINPUT {
        wVk: VK_SPACE,
        wScan: 0,
        dwFlags: KEYBD_EVENT_FLAGS(0),
        time: 0,
        dwExtraInfo: 0,
    };
    input_down.r#type = INPUT_KEYBOARD;
    input_down.Anonymous.ki = key_input;

    let result_down = unsafe {
        windows::Win32::UI::Input::KeyboardAndMouse::SendInput(
            &[input_down],
            std::mem::size_of::<INPUT>() as i32,
        )
    };

    if result_down == 1 {
        println!("SPACE 键按下成功！");
    } else {
        println!("SPACE 键按下失败！");
    }

    Ok(())
}

// pub type HOOKPROC = Option<unsafe extern "system" fn(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT>;
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if wparam == WPARAM(WM_LBUTTONDOWN as usize) {
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
            println!("鼠标左键按下啦 {} ", time);
        }

        if wparam == WPARAM(WM_LBUTTONUP as usize) {
            println!("鼠标左键松开啦");
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}
