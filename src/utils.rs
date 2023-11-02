use std::error::Error;
use std::time::UNIX_EPOCH;
use std::u8;

use windows::core::HSTRING;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat};
use windows::Storage::{CreationCollisionOption, FileAccessMode, StorageFolder};

pub fn may_sort_asc<T>(vec: &mut Vec<T>)
where
    T: PartialOrd + Ord,
{
    for i in 1..vec.len() {
        if vec[i] < vec[i - 1] {
            vec.sort();
        }
    }
}

pub fn save_buffer_to_image(
    width: u32,
    height: u32,
    buffer: Vec<u8>,
) -> Result<(), Box<dyn Error>> {
    let path = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let path = path + "\\temp";
    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(&path))?.get()?;

    let now = std::time::SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    let filename = format!("screenshot-{}.png", since_the_epoch.as_millis());
    let file = folder
        .CreateFileAsync(
            &HSTRING::from(filename),
            CreationCollisionOption::ReplaceExisting,
        )?
        .get()?;

    let stream = file.OpenAsync(FileAccessMode::ReadWrite)?.get()?;
    let encoder = BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId()?, &stream)?.get()?;
    encoder.SetPixelData(
        BitmapPixelFormat::Bgra8,
        BitmapAlphaMode::Premultiplied,
        width,
        height,
        1.0,
        1.0,
        &buffer,
    )?;

    encoder.FlushAsync()?.get()?;

    Ok(())
}
