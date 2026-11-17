//! "Text from screen": capture a screen area (GDI) and read it with the built-in Windows OCR (offline, free).

use anyhow::{anyhow, Result};
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Graphics::Gdi::*;

/// Capture a rectangle of the virtual screen (physical pixels). Returns BGRA bytes.
pub fn capture(x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>> {
    if w < 2 || h < 2 {
        return Err(anyhow!("Area too small"));
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY)?;
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        for px in buf.chunks_mut(4) {
            px[3] = 255;
        }
        Ok(buf)
    }
}

/// Scale small images up so the OCR engine can read small text.
fn upscale(bgra: Vec<u8>, w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    if w >= 600 && h >= 120 {
        return (bgra, w, h);
    }
    let f = 2;
    let (nw, nh) = (w * f, h * f);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let si = (((y / f) * w + (x / f)) * 4) as usize;
            let di = ((y * nw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&bgra[si..si + 4]);
        }
    }
    (out, nw, nh)
}

/// Read text from a BGRA image.
pub fn recognize(bgra: Vec<u8>, w: u32, h: u32) -> Result<String> {
    let (bgra, w, h) = upscale(bgra, w, h);
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bmp = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        w as i32,
        h as i32,
        BitmapAlphaMode::Premultiplied,
    )?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|_| anyhow!("Windows OCR is not available. Add a language with OCR support in Windows Settings → Language."))?;
    let result = engine.RecognizeAsync(&bmp)?.get()?;
    let mut lines = vec![];
    for line in result.Lines()? {
        lines.push(line.Text()?.to_string());
    }
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        return Err(anyhow!("No readable text found in that area."));
    }
    Ok(text)
}

pub fn rgba_to_bgra(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.chunks_mut(4) {
        px.swap(0, 2);
    }
    rgba
}
