//! Explicit user paste and generated safe diagnostic copy only. Native file
//! URLs and complete path text are resolved here, never accepted from WebUI.
//! Never read HTML, fetch remote content, or evaluate shell syntax.
//! ImageIO metadata bounds decoding before AppKit converts TIFF/JPEG to PNG.
use agenthub_core::codex::desktop::attachments::MAX_IMAGE_BYTES;
use agenthub_core::codex::desktop::references::NativeReference;
use objc2::{rc::autoreleasepool, runtime::AnyObject};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardTypeFileURL,
    NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
};
use objc2_foundation::{NSData, NSDictionary, NSString};
use std::ffi::c_void;
use std::path::{Path, PathBuf};

const CAPTURE_BYTES: usize = 64 * 1024 * 1024;
const PIXEL_BYTES: u64 = 128 * 1024 * 1024;
const INVALID: &str = "剪贴板图片格式无效或不受支持";
const TOO_LARGE: &str = "剪贴板图片超出安全大小限制";
const TEXT_BYTES: usize = 32 * 8192;

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod ingress_tests;

/// AppKit pasteboard and image conversion are not safe to drive from several
/// test threads at once: concurrent use intermittently returned another
/// test's items or raised a foreign exception that aborted the test process.
#[cfg(test)]
fn appkit_test_pool<R>(test: impl FnOnce() -> R) -> R {
    static APPKIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = APPKIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    autoreleasepool(|_| test())
}

pub(super) enum PasteContent {
    Files(Vec<NativeReference>),
    Image(Vec<u8>),
    Text,
    Unsupported,
}

pub(super) struct PasteCapture {
    pub change_count: isize,
    pub content: PasteContent,
}

pub(super) fn capture() -> Result<PasteCapture, &'static str> {
    autoreleasepool(|_| {
        read_paste(
            &NSPasteboard::generalPasteboard(),
            dirs::home_dir().as_deref(),
        )
    })
}

pub(super) fn unchanged(change_count: isize) -> bool {
    autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount() == change_count)
}

fn read_paste(board: &NSPasteboard, home_dir: Option<&Path>) -> Result<PasteCapture, &'static str> {
    let before = board.changeCount();
    let content = read_content(board, home_dir)?;
    if before != board.changeCount() {
        return Err("剪贴板已变化，请重新粘贴");
    }
    Ok(PasteCapture {
        change_count: before,
        content,
    })
}

fn read_content(
    board: &NSPasteboard,
    home_dir: Option<&Path>,
) -> Result<PasteContent, &'static str> {
    // Finder may also advertise a TIFF preview. File identities take precedence
    // so a copied document never turns into its icon or thumbnail.
    let mut paths = Vec::new();
    if let Some(items) = board.pasteboardItems() {
        if items.len() > 32 {
            return Err("引用数量超出限制，本次附件未添加");
        }
        for item in items.iter() {
            // SAFETY: immutable AppKit pasteboard type constant.
            if let Some(value) = unsafe { item.stringForType(NSPasteboardTypeFileURL) } {
                if value.length() > 8192 {
                    return Err("文件名或路径格式无效");
                }
                paths.push(
                    local_file_url(&value.to_string())
                        .ok_or("仅支持本机文件，无法添加此文件地址")?,
                );
            }
        }
    }
    if !paths.is_empty() {
        return selected_files(paths).map(PasteContent::Files);
    }
    match read_png(board) {
        Ok(image) => return Ok(PasteContent::Image(image)),
        Err("剪贴板没有可粘贴的图片") => {}
        Err(error) => return Err(error),
    }
    // SAFETY: immutable AppKit text type. Text never crosses back to WebUI;
    // the browser retains its original paste data for the ordinary text path.
    if let Some(text) = unsafe { board.stringForType(NSPasteboardTypeString) } {
        if text.length() <= TEXT_BYTES {
            let text = text.to_string();
            if let Some(paths) = text_paths(&text, home_dir) {
                // Only an entirely valid set becomes attachments. Prose or an
                // unavailable path remains text; never consume its valid prefix.
                if let Ok(files) = selected_files(paths) {
                    return Ok(PasteContent::Files(files));
                }
            }
        }
        return Ok(PasteContent::Text);
    }
    Ok(PasteContent::Unsupported)
}

fn local_file_url(value: &str) -> Option<PathBuf> {
    let url = tauri::Url::parse(value).ok()?;
    if url.scheme() != "file"
        || url.host_str().is_some_and(|host| host != "localhost")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    url.to_file_path().ok()
}

fn text_paths(text: &str, home_dir: Option<&Path>) -> Option<Vec<PathBuf>> {
    if text.len() > TEXT_BYTES {
        return None;
    }
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if paths.len() == 32 {
            return None;
        }
        let line = line
            .strip_prefix('"')
            .and_then(|line| line.strip_suffix('"'))
            .or_else(|| {
                line.strip_prefix('\'')
                    .and_then(|line| line.strip_suffix('\''))
            })
            .unwrap_or(line);
        if line.len() > 8192 || line.chars().any(char::is_control) {
            return None;
        }
        let path = if line.starts_with("file:") {
            local_file_url(line)?
        } else if let Some(relative) = line.strip_prefix("~/") {
            home_dir?.join(relative)
        } else if line.starts_with('/') {
            PathBuf::from(line)
        } else {
            return None;
        };
        paths.push(path);
    }
    (!paths.is_empty()).then_some(paths)
}

pub(super) fn selected_files(paths: Vec<PathBuf>) -> Result<Vec<NativeReference>, &'static str> {
    if paths.is_empty() || paths.len() > 32 {
        return Err("引用数量超出限制，本次附件未添加");
    }
    let mut files = Vec::new();
    for path in paths {
        let file = NativeReference::selected_file_detailed(path).map_err(|(_, message)| message)?;
        if !files
            .iter()
            .any(|existing: &NativeReference| existing.same_selected_file(&file))
        {
            files.push(file);
        }
    }
    Ok(files)
}

pub(super) fn copy_diagnostics(
    diagnostics: &agenthub_core::services::codex_desktop::DesktopDiagnostics,
) -> Result<(), &'static str> {
    autoreleasepool(|_| write_diagnostics(&NSPasteboard::generalPasteboard(), diagnostics))
}

fn write_diagnostics(
    pasteboard: &NSPasteboard,
    diagnostics: &agenthub_core::services::codex_desktop::DesktopDiagnostics,
) -> Result<(), &'static str> {
    let text = serde_json::to_string_pretty(diagnostics).map_err(|_| "无法复制诊断，请重试")?;
    if text.len() > 64 * 1024 {
        return Err("诊断超出复制容量");
    }
    pasteboard.clearContents();
    // SAFETY: immutable AppKit text type; content is generated from the bounded
    // diagnostic DTO, never a caller-supplied string or native error payload.
    if unsafe { pasteboard.setString_forType(&NSString::from_str(&text), NSPasteboardTypeString) } {
        Ok(())
    } else {
        Err("无法复制诊断，请重试")
    }
}

fn read_png(pasteboard: &NSPasteboard) -> Result<Vec<u8>, &'static str> {
    read_png_using(pasteboard, normalize)
}

fn read_png_using(
    pasteboard: &NSPasteboard,
    convert: impl FnOnce(&[u8], bool) -> Result<Vec<u8>, &'static str>,
) -> Result<Vec<u8>, &'static str> {
    let before = pasteboard.changeCount();
    // SAFETY: AppKit exports these immutable pasteboard type constants.
    let (data, png) = unsafe {
        if let Some(data) = pasteboard.dataForType(NSPasteboardTypePNG) {
            (data, true)
        } else if let Some(data) = pasteboard.dataForType(&NSString::from_str("public.jpeg")) {
            (data, false)
        } else {
            (
                pasteboard
                    .dataForType(NSPasteboardTypeTIFF)
                    .ok_or("剪贴板没有可粘贴的图片")?,
                false,
            )
        }
    };
    if data.len() > CAPTURE_BYTES {
        return Err(TOO_LARGE);
    }
    let bytes = data.to_vec();
    if before != pasteboard.changeCount() {
        return Err("剪贴板已变化，请重新粘贴");
    }
    let result = convert(&bytes, png)?;
    if before != pasteboard.changeCount() {
        return Err("剪贴板已变化，请重新粘贴");
    }
    Ok(result)
}

fn normalize(bytes: &[u8], png: bool) -> Result<Vec<u8>, &'static str> {
    if bytes.len() > CAPTURE_BYTES {
        return Err(TOO_LARGE);
    }
    if bytes.is_empty() {
        return Err(INVALID);
    }
    check_dimensions(bytes)?;
    if png {
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err("粘贴图片超过 20MiB");
        }
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(INVALID);
        }
        return Ok(bytes.to_vec());
    }
    let data = NSData::with_bytes(bytes);
    let bitmap = NSBitmapImageRep::imageRepWithData(&data).ok_or(INVALID)?;
    // SAFETY: the empty dictionary has the documented property-key/object type;
    // PNG is lossless and no scaling/quality properties are supplied.
    let png = unsafe {
        bitmap.representationUsingType_properties(
            NSBitmapImageFileType::PNG,
            &NSDictionary::<objc2_foundation::NSString, AnyObject>::new(),
        )
    }
    .ok_or(INVALID)?;
    if png.len() > MAX_IMAGE_BYTES {
        return Err("粘贴图片超过 20MiB");
    }
    Ok(png.to_vec())
}

// Narrow system-framework FFI avoids adding an image codec/runtime dependency.
type CF = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: CF, bytes: *const u8, length: isize) -> CF;
    fn CFRelease(value: CF);
    fn CFDictionaryGetValue(dictionary: CF, key: CF) -> CF;
    fn CFNumberGetValue(number: CF, kind: isize, value: *mut c_void) -> bool;
    fn CFGetTypeID(value: CF) -> usize;
    fn CFNumberGetTypeID() -> usize;
}
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: CF, options: CF) -> CF;
    fn CGImageSourceGetCount(source: CF) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: CF, index: usize, options: CF) -> CF;
    static kCGImagePropertyPixelWidth: CF;
    static kCGImagePropertyPixelHeight: CF;
    static kCGImagePropertyDepth: CF;
}
struct OwnedCF(CF);
impl OwnedCF {
    fn new(value: CF) -> Result<Self, &'static str> {
        if value.is_null() {
            Err(INVALID)
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for OwnedCF {
    fn drop(&mut self) {
        // SAFETY: every instance owns one non-null Create/Copy CF reference.
        unsafe { CFRelease(self.0) }
    }
}
fn check_dimensions(bytes: &[u8]) -> Result<(), &'static str> {
    // SAFETY: buffers live for these synchronous calls, owned CF values are
    // released in reverse order, and numeric types are checked before reading.
    unsafe {
        let data = OwnedCF::new(CFDataCreate(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
        ))?;
        let source = OwnedCF::new(CGImageSourceCreateWithData(data.0, std::ptr::null()))?;
        // Clipboard images are a single bitmap; don't decode unbounded frames.
        if CGImageSourceGetCount(source.0) != 1 {
            return Err(INVALID);
        }
        let properties = OwnedCF::new(CGImageSourceCopyPropertiesAtIndex(
            source.0,
            0,
            std::ptr::null(),
        ))?;
        let number = |key| -> Result<u64, &'static str> {
            let value = CFDictionaryGetValue(properties.0, key);
            if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
                return Err(INVALID);
            }
            let mut number = 0i64;
            // kCFNumberSInt64Type = 4 in the system CoreFoundation header.
            if !CFNumberGetValue(value, 4, (&mut number as *mut i64).cast()) || number <= 0 {
                return Err(INVALID);
            }
            Ok(number as u64)
        };
        let width = number(kCGImagePropertyPixelWidth)?;
        let height = number(kCGImagePropertyPixelHeight)?;
        if number(kCGImagePropertyDepth)? > 16 {
            return Err(INVALID);
        }
        // Reserve sixteen bytes per pixel for decoded/conversion surfaces.
        if width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(16))
            .is_none_or(|bytes| bytes > PIXEL_BYTES)
        {
            return Err(TOO_LARGE);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenthub_core::services::codex_desktop::{DesktopDiagnostic, DesktopDiagnostics};
    pub(super) const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 96, 248, 15,
        0, 1, 4, 1, 0, 95, 229, 195, 75, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    #[test]
    fn png_is_preserved_and_tiff_is_converted_without_resizing() {
        appkit_test_pool(|| {
            assert_eq!(normalize(PNG, true).unwrap(), PNG);
            let bitmap = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(PNG)).unwrap();
            // SAFETY: documented property types and an empty property dictionary.
            let tiff = unsafe {
                bitmap.representationUsingType_properties(
                    NSBitmapImageFileType::TIFF,
                    &NSDictionary::<objc2_foundation::NSString, AnyObject>::new(),
                )
            }
            .unwrap();
            let converted = normalize(&tiff.to_vec(), false).unwrap();
            assert!(converted.starts_with(b"\x89PNG\r\n\x1a\n"));
            let bitmap =
                NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&converted)).unwrap();
            assert_eq!((bitmap.pixelsWide(), bitmap.pixelsHigh()), (1, 1));
            assert!(normalize(b"not an image", true).is_err());
        });
    }

    #[test]
    fn private_pasteboard_changed_during_conversion_is_rejected() {
        appkit_test_pool(|| {
            // A unique test pasteboard never reads or changes the user's clipboard.
            let board = NSPasteboard::pasteboardWithUniqueName();
            board.clearContents();
            // SAFETY: immutable AppKit type constant, fixture NSData retained.
            assert!(unsafe {
                board.setData_forType(Some(&NSData::with_bytes(PNG)), NSPasteboardTypePNG)
            });
            assert_eq!(read_png(&board).unwrap(), PNG);
            let result = read_png_using(&board, |bytes, png| {
                board.clearContents();
                normalize(bytes, png)
            });
            assert_eq!(result.unwrap_err(), "剪贴板已变化，请重新粘贴");
            assert_eq!(read_png(&board).unwrap_err(), "剪贴板没有可粘贴的图片");
        });
    }

    fn diagnostics_with_hint(hint: String) -> DesktopDiagnostics {
        DesktopDiagnostics {
            records: vec![DesktopDiagnostic {
                timestamp: 1,
                source: "fixture-source".into(),
                method: "fixture-method".into(),
                version: Some(1),
                code: None,
                category: "fixture-category".into(),
                hint,
            }],
            desktop_build: Some("fixture-build".into()),
            bundled_cli_version: None,
        }
    }

    #[test]
    fn diagnostic_copy_writes_safe_json_to_a_private_pasteboard() {
        appkit_test_pool(|| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            let diagnostics = diagnostics_with_hint("fixture diagnostic".into());
            let expected = serde_json::to_string_pretty(&diagnostics).unwrap();

            write_diagnostics(&board, &diagnostics).unwrap();

            // SAFETY: AppKit exports this immutable pasteboard type constant.
            assert_eq!(
                unsafe { board.stringForType(NSPasteboardTypeString) }
                    .expect("diagnostic string is present")
                    .to_string(),
                expected
            );
        });
    }

    #[test]
    fn oversized_diagnostic_copy_keeps_private_pasteboard_contents() {
        appkit_test_pool(|| {
            let board = NSPasteboard::pasteboardWithUniqueName();
            let existing = NSString::from_str("fixture-existing-content");
            // SAFETY: AppKit exports this immutable pasteboard type constant.
            assert!(unsafe { board.setString_forType(&existing, NSPasteboardTypeString) });
            let change_count = board.changeCount();
            let diagnostics = diagnostics_with_hint("x".repeat(64 * 1024));

            assert_eq!(
                write_diagnostics(&board, &diagnostics),
                Err("诊断超出复制容量")
            );
            assert_eq!(board.changeCount(), change_count);
            // SAFETY: AppKit exports this immutable pasteboard type constant.
            assert_eq!(
                unsafe { board.stringForType(NSPasteboardTypeString) }
                    .expect("existing content remains present")
                    .to_string(),
                "fixture-existing-content"
            );
        });
    }

    #[test]
    fn oversized_capture_is_rejected_before_image_decoding() {
        let bytes = vec![0; CAPTURE_BYTES + 1];
        assert_eq!(normalize(&bytes, true).unwrap_err(), TOO_LARGE);
    }
}
