#![allow(unused_imports)]
use core::slice;
use std::error::Error;
use std::f32::consts::PI;
use std::ptr::null_mut;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{thread, time, u8};

use opencv::core::{Mat, Point, Point2f, Scalar, BORDER_CONSTANT};
use opencv::imgproc;
use opencv::prelude::*;
use opencv::types::VectorOfu8;

use windows::core::Interface;
use windows::core::HSTRING;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat};
use windows::Storage::{CreationCollisionOption, FileAccessMode, StorageFolder};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{self, XFORM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    self, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MAP_VIRTUAL_KEY_TYPE, VIRTUAL_KEY, VK_LBUTTON, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    self, GWLP_USERDATA, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WINDOWS_HOOK_ID, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP,
};

mod utils;

#[derive(Debug, Clone)]
struct ArcZone {
    segment: Vec<u32>,
}

impl ArcZone {
    fn new() -> ArcZone {
        ArcZone { segment: vec![] }
    }
    fn is_include(&self, other: &Vec<u32>) -> bool {
        let other_len = other.len();
        let seg_len = self.segment.len();
        if seg_len < 2 {
            return false;
        }

        if self.segment[0] - 6 <= other[0] && self.segment[seg_len - 1] >= other[other_len - 1] {
            return true;
        }
        false
    }
}

fn main() {
    let hwnd = unsafe { WindowsAndMessaging::GetDesktopWindow() };
    let mut arc_zone = ArcZone::new();
    let handle = thread::spawn(move || loop {
        screenshot_by_hwnd(hwnd, &mut arc_zone).unwrap();
    });

    handle.join().unwrap();
}

fn screenshot_by_hwnd(hwnd: HWND, arc_zone: &mut ArcZone) -> Result<(), Box<dyn Error>> {
    Ok(unsafe {
        let hdc = Gdi::GetWindowDC(hwnd);
        let mut rect = RECT::default();
        WindowsAndMessaging::GetClientRect(hwnd, &mut rect);

        // default 2560 x 1440
        let mut radius = 87;
        let mut taskbar_height = 20;

        // 1920 * 1080
        if rect.right == 1920 {
            radius = 67;
            taskbar_height = 15;
        }

        let diameter = radius * 2;
        let capture_x = (rect.right - rect.left) / 2 - radius;
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

        let r = radius;
        let center_x = r;
        let center_y = r;
        let num_points = 360;
        let max_len = buffer.len() as i32;
        let mut white_zone: Vec<u32> = vec![];
        let mut red_zone: Vec<u32> = vec![];

        // 设置中间圆心为蓝色
        buffer[((center_y * width + center_x) * 4) as usize] = 255;
        buffer[((center_y * width + center_x) * 4 + 1) as usize] = 0;
        buffer[((center_y * width + center_x) * 4 + 2) as usize] = 0;
        buffer[((center_y * width + center_x) * 4 + 3) as usize] = 255;

        let mut img = Mat::new_rows_cols_with_data(
            width,
            width,
            opencv::core::CV_8UC4,
            buffer.as_mut_ptr() as *mut std::ffi::c_void,
            opencv::core::Mat_AUTO_STEP,
        )?;

        for angle in 0..num_points {
            let angle_rad = (angle as f32) * 2.0 * PI / (num_points as f32);
            let x = (center_x as f32) + (r as f32) * angle_rad.cos();
            let y = (center_y as f32) + (r as f32) * angle_rad.sin();

            let x = x.floor() as i32;
            let y = y.floor() as i32;

            if x == width || y == width {
                continue;
            };

            let dest_index = (y * width + x) * 4;
            if dest_index < max_len {
                let red = buffer[(dest_index + 2) as usize];
                let green = buffer[(dest_index + 1) as usize];
                let blue = buffer[(dest_index) as usize];

                // 类白色
                if red > 240 && green > 240 && blue > 240 {
                    imgproc::line(
                        &mut img,
                        Point { x, y },
                        Point { x: r, y: r },
                        Scalar::new(0.0, 255.0, 0.0, 255.0),
                        1,
                        8,
                        0,
                    )?;

                    match map_arc_len(x, y, r) {
                        Some(l) => white_zone.push(l as u32),
                        None => (),
                    };
                }

                // 类红色
                if red > 200 && green < 50 && blue < 50 {
                    imgproc::line(
                        &mut img,
                        Point { x, y },
                        Point { x: r, y: r },
                        Scalar::new(255.0, 0.0, 0.0, 255.0),
                        1,
                        8,
                        0,
                    )?;

                    match map_arc_len(x, y, r) {
                        Some(l) => red_zone.push(l as u32),
                        None => (),
                    };
                }
            }
        }

        let arc_len = arc_zone.segment.len();
        let red_len = red_zone.len();

        white_zone.retain(|x| *x > 0);

        utils::may_sort_asc(&mut white_zone);
        utils::may_sort_asc(&mut red_zone);

        if red_len == 0 {
            arc_zone.segment = vec![];
        } else if arc_len == 0 {
            arc_zone.segment = white_zone;
        }

        if red_len > 0 && arc_zone.is_include(&red_zone) {
            press_space().unwrap();
            // let img_data_ptr = img.data();
            // let img_data_size = (img.rows() * img.cols() * img.elem_size()? as i32) as usize;

            // let img_vec: Vec<u8> = slice::from_raw_parts(img_data_ptr, img_data_size).to_vec();
            // let _ = utils::save_buffer_to_image(width as u32, width as u32, img_vec);
        }
    })
}

fn press_space() -> windows::core::Result<()> {
    let mut input_down: INPUT = INPUT::default();
    let key_input = KEYBDINPUT {
        wVk: VIRTUAL_KEY(0),
        wScan: 0x39,
        dwFlags: KEYEVENTF_SCANCODE,
        time: 0,
        dwExtraInfo: 0,
    };
    input_down.r#type = INPUT_KEYBOARD;
    input_down.Anonymous.ki = key_input;

    let result_down = unsafe {
        KeyboardAndMouse::SendInput(&[input_down], std::mem::size_of::<INPUT>() as i32);

        input_down.Anonymous.ki.dwFlags = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;

        KeyboardAndMouse::SendInput(&[input_down], std::mem::size_of::<INPUT>() as i32)
    };

    if result_down == 1 {
        // println!("SPACE 键按下成功！");
    } else {
        // println!("SPACE 键按下失败！");
    }

    Ok(())
}

fn map_arc_len(mut x: i32, mut y: i32, r: i32) -> Option<f64> {
    if x >= r && y <= r {
        // 1
        x = x - r;
        y = r - y;
    } else if x >= r && y >= r {
        // 4
        x = x - r;
        y = -(y - r);
    } else if x <= r && y >= r {
        // 3
        x = -(r - x);
        y = -(y - r);
    } else if x <= r && y <= r {
        // 2
        x = -(r - x);
        y = r - y;
    }
    let angle_rad = f64::atan2(x as f64, y as f64);

    let clockwise_angle_rad = if angle_rad >= 0.0 {
        angle_rad
    } else {
        2.0 * std::f64::consts::PI + angle_rad
    };

    Some((r as f64) * clockwise_angle_rad)
}

// pub type HOOKPROC = Option<unsafe extern "system" fn(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT>;
