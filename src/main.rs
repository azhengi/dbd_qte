#![allow(unused_imports)]
use core::slice;
use std::error::Error;
use std::f32::consts::PI;
use std::ptr::null_mut;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{thread, time, u8};

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

#[derive(Debug, Clone)]
struct Rad {
    hit_zone: Vec<u32>,
}

fn main() {
    let hwnd = unsafe { WindowsAndMessaging::GetDesktopWindow() };
    let mut rad = Rad { hit_zone: vec![] };

    let handle = thread::spawn(move || loop {
        let duration = time::Duration::from_millis(30);
        thread::sleep(duration);
        if unsafe { GetAsyncKeyState(1) } != 0 {
            screenshot_by_hwnd(hwnd, &mut rad).unwrap();
        }
    });

    handle.join().unwrap();
}

fn screenshot_by_hwnd(hwnd: HWND, rad: &mut Rad) -> Result<(), Box<dyn Error>> {
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
        let prev_rad = rad.clone();

        let now = std::time::SystemTime::now();

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

        for i in 0..num_points {
            let angle = (i as f32) * 2.0 * PI / (num_points as f32);

            let x = (center_x as f32) + (r as f32) * angle.cos();
            let y = (center_y as f32) + (r as f32) * angle.sin();
            let mut x = x.floor() as i32;
            let mut y = y.floor() as i32;

            let dest_index = (y * width + x) * 4;
            if dest_index < max_len {
                let red = buffer[(dest_index + 2) as usize];
                let green = buffer[(dest_index + 1) as usize];
                let blue = buffer[(dest_index) as usize];

                if x > r && y < r {
                    x = x - r;
                    y = r - y;
                }
                if x > r && y > r {
                    x = x - r;
                    y = y - r;
                }
                if x < r && y > r {
                    x = r - x;
                    y = y - r;
                }
                if x < r && y < r {
                    x = r - x;
                    y = r - y;
                }

                // 类白色
                if red > 240 && green > 240 && blue > 240 {
                    let theta = 2.0
                        * (((x - 0).pow(2) as f64 + (y - r).pow(2) as f64).sqrt()
                            / (2.0 * r as f64))
                            .asin();

                    let l = (2.0 * (r as f32) * (theta as f32) * PI);
                    white_zone.push(l as u32);
                    println!("白色弧长 {}", l);
                }

                // 类红色
                if red > 200 && green < 50 && blue < 50 {
                    let theta = 2.0
                        * (((x - 0).pow(2) as f64 + (y - r).pow(2) as f64).sqrt()
                            / (2.0 * r as f64))
                            .asin();

                    let l = (2.0 * (r as f32) * (theta as f32) * PI);
                    red_zone.push(l as u32);
                    println!("红色弧长 {}", l);
                }
            }
        }

        if red_zone.len() == 0 {
            rad.hit_zone = vec![];
        }

        if red_zone.len() != 0 && rad.hit_zone.len() == 0 {
            rad.hit_zone = white_zone;
        }

        if rad.hit_zone.len() >= 2 {
            let first_index = 0;
            let hit_last_index = rad.hit_zone.len() - 1;
            let last_index = red_zone.len() - 1;

            if rad.hit_zone[hit_last_index] < rad.hit_zone[first_index] {
                rad.hit_zone.sort();
            }
            if red_zone[last_index] < red_zone[first_index] {
                red_zone.sort();
            }

            if rad.hit_zone[first_index] < red_zone[first_index]
                && rad.hit_zone[hit_last_index] > red_zone[last_index]
            {
                println!("命中了阿");
                // let _ = press_space();
                let _ = save_buffer_to_image(width as u32, width as u32, &buffer);
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
