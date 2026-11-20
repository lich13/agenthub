//! Explicit user paste and generated safe diagnostic copy only. Native file
//! URLs and complete path text are resolved here, never accepted from UI JSON.
//! Never read HTML, fetch remote content, or evaluate shell syntax.
//! ImageIO metadata bounds decoding and lossless PNG transcoding stay 100% in Rust.
use crate::host::HostBridge;
use agenthub_core::codex::desktop::attachments::MAX_IMAGE_BYTES;
use agenthub_core::codex::desktop::references::NativeReference;
use std::ffi::c_char;
#[cfg(target_os = "macos")]
use std::ffi::c_void;
use std::path::{Path, PathBuf};

pub(crate) const CAPTURE_BYTES: usize = 64 * 1024 * 1024;
const PIXEL_BYTES: u64 = 128 * 1024 * 1024;
const INVALID: &str = "剪贴板图片格式无效或不受支持";
const TOO_LARGE: &str = "剪贴板图片超出安全大小限制";
pub(crate) const MAX_FILES: usize = 32;
pub(crate) const MAX_PATH_UNITS: usize = 8192;
pub(crate) const TEXT_BYTES: usize = MAX_FILES * MAX_PATH_UNITS;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentHubRawImageKind {
    None = 0,
    Png = 1,
    Jpeg = 2,
    Tiff = 3,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AgentHubRawFileUrl {
    pub utf8_ptr: *const c_char,
    pub utf8_len: usize,
    pub utf16_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AgentHubRawPasteboard {
    pub change_count_before: i64,
    pub change_count_after: i64,
    pub total_item_count: usize,
    pub file_urls_ptr: *const AgentHubRawFileUrl,
    pub file_urls_len: usize,
    pub image_kind: AgentHubRawImageKind,
    pub image_declared_bytes: usize,
    pub image_bytes_ptr: *const u8,
    pub image_bytes_len: usize,
    pub has_string: bool,
    pub string_utf16_len: usize,
    pub string_utf8_ptr: *const c_char,
    pub string_utf8_len: usize,
}

#[derive(Debug, Clone)]
pub struct RawFileUrlItem {
    pub url: String,
    pub utf16_len: usize,
}

#[derive(Debug, Clone)]
pub struct RawPasteboardSnapshot {
    pub change_count_before: i64,
    pub change_count_after: i64,
    pub total_item_count: usize,
    pub file_urls: Vec<RawFileUrlItem>,
    pub image_kind: AgentHubRawImageKind,
    pub image_declared_bytes: usize,
    pub image_bytes: Vec<u8>,
    pub has_string: bool,
    pub string_utf16_len: usize,
    pub string_text: Option<String>,
}

impl RawPasteboardSnapshot {
    /// # Safety
    /// Caller must ensure `raw` and any non-zero-length slice pointers are valid
    /// for the duration of this call. Zero-length slices never dereference pointers.
    pub unsafe fn from_raw(raw: &AgentHubRawPasteboard) -> Result<Self, &'static str> {
        if raw.total_item_count > MAX_FILES || raw.file_urls_len > MAX_FILES {
            return Err("引用数量超出限制，本次附件未添加");
        }
        let mut file_urls = Vec::new();
        if raw.file_urls_len > 0 {
            if raw.file_urls_ptr.is_null() {
                return Err("文件名或路径格式无效");
            }
            let items =
                // SAFETY: The checked count describes the host's contiguous initialized file URL descriptors.
                unsafe { std::slice::from_raw_parts(raw.file_urls_ptr, raw.file_urls_len) };
            for item in items {
                if item.utf16_len > MAX_PATH_UNITS
                    || item.utf8_len > item.utf16_len.saturating_mul(3)
                {
                    return Err("文件名或路径格式无效");
                }
                let url = if item.utf8_len == 0 {
                    String::new()
                } else {
                    if item.utf8_ptr.is_null() {
                        return Err("文件名或路径格式无效");
                    }
                    // SAFETY: The URL descriptor borrows this checked byte range from the live pasteboard snapshot.
                    let bytes = unsafe {
                        std::slice::from_raw_parts(item.utf8_ptr.cast::<u8>(), item.utf8_len)
                    };
                    std::str::from_utf8(bytes)
                        .map_err(|_| "文件名或路径格式无效")?
                        .to_owned()
                };
                file_urls.push(RawFileUrlItem {
                    url,
                    utf16_len: item.utf16_len,
                });
            }
        }
        if raw.image_bytes_len > CAPTURE_BYTES
            || (raw.image_bytes_len != 0 && raw.image_bytes_len != raw.image_declared_bytes)
        {
            return Err(TOO_LARGE);
        }
        if raw.string_utf8_len > raw.string_utf16_len.saturating_mul(3) {
            return Err(INVALID);
        }
        let image_bytes = if raw.image_bytes_len == 0 || raw.image_declared_bytes > CAPTURE_BYTES {
            Vec::new()
        } else {
            if raw.image_bytes_ptr.is_null() {
                return Err(INVALID);
            }
            // SAFETY: The host lends the validated image byte range until it is copied into this Vec.
            unsafe { std::slice::from_raw_parts(raw.image_bytes_ptr, raw.image_bytes_len) }.to_vec()
        };
        let string_text = if !raw.has_string || raw.string_utf16_len > TEXT_BYTES {
            None
        } else if raw.string_utf8_len == 0 {
            Some(String::new())
        } else if raw.string_utf8_ptr.is_null() {
            None
        } else {
            // SAFETY: The checked length describes bytes in the live host string buffer.
            let bytes = unsafe {
                std::slice::from_raw_parts(raw.string_utf8_ptr.cast::<u8>(), raw.string_utf8_len)
            };
            std::str::from_utf8(bytes).ok().map(ToOwned::to_owned)
        };
        Ok(Self {
            change_count_before: raw.change_count_before,
            change_count_after: raw.change_count_after,
            total_item_count: raw.total_item_count,
            file_urls,
            image_kind: raw.image_kind,
            image_declared_bytes: raw.image_declared_bytes,
            image_bytes,
            has_string: raw.has_string,
            string_utf16_len: raw.string_utf16_len,
            string_text,
        })
    }
}

pub(super) enum PasteContent {
    Files(Vec<NativeReference>),
    Image(Vec<u8>),
    Text,
    Unsupported,
}

pub(super) struct PasteCapture {
    pub change_count: i64,
    pub content: PasteContent,
}

pub(super) fn read_paste(
    snapshot: &RawPasteboardSnapshot,
    home_dir: Option<&Path>,
) -> Result<PasteCapture, &'static str> {
    if snapshot.change_count_before != snapshot.change_count_after {
        return Err("剪贴板已变化，请重新粘贴");
    }
    let content = read_content(snapshot, home_dir)?;
    Ok(PasteCapture {
        change_count: snapshot.change_count_before,
        content,
    })
}

pub(super) fn read_content(
    snapshot: &RawPasteboardSnapshot,
    home_dir: Option<&Path>,
) -> Result<PasteContent, &'static str> {
    if snapshot.total_item_count > MAX_FILES || snapshot.file_urls.len() > MAX_FILES {
        return Err("引用数量超出限制，本次附件未添加");
    }
    let mut paths = Vec::new();
    for item in &snapshot.file_urls {
        if item.utf16_len > MAX_PATH_UNITS {
            return Err("文件名或路径格式无效");
        }
        paths.push(local_file_url(&item.url).ok_or("仅支持本机文件，无法添加此文件地址")?);
    }
    if !paths.is_empty() {
        return selected_files(paths).map(PasteContent::Files);
    }

    if snapshot.image_kind != AgentHubRawImageKind::None {
        if snapshot.image_declared_bytes > CAPTURE_BYTES {
            return Err(TOO_LARGE);
        }
        let is_png = snapshot.image_kind == AgentHubRawImageKind::Png;
        let png = normalize(&snapshot.image_bytes, is_png)?;
        return Ok(PasteContent::Image(png));
    }

    if snapshot.has_string {
        if snapshot.string_utf16_len <= TEXT_BYTES {
            if let Some(text) = snapshot.string_text.as_deref() {
                if let Some(paths) = text_paths(text, home_dir) {
                    if let Ok(files) = selected_files(paths) {
                        return Ok(PasteContent::Files(files));
                    }
                }
            }
        }
        return Ok(PasteContent::Text);
    }

    Ok(PasteContent::Unsupported)
}

pub(super) fn local_file_url(value: &str) -> Option<PathBuf> {
    let url = url::Url::parse(value).ok()?;
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
        if line.len() > MAX_PATH_UNITS || line.chars().any(char::is_control) {
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
    if paths.is_empty() || paths.len() > MAX_FILES {
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

pub(super) fn format_diagnostics(
    diagnostics: &agenthub_core::services::codex_desktop::DesktopDiagnostics,
) -> Result<String, &'static str> {
    let text = serde_json::to_string_pretty(diagnostics).map_err(|_| "无法复制诊断，请重试")?;
    if text.len() > 64 * 1024 {
        return Err("诊断超出复制容量");
    }
    Ok(text)
}

pub(super) async fn copy_diagnostics(
    host: &HostBridge,
    diagnostics: &agenthub_core::services::codex_desktop::DesktopDiagnostics,
) -> Result<(), &'static str> {
    let text = format_diagnostics(diagnostics)?;
    host.write_pasteboard(&text).await
}

pub(super) fn normalize(bytes: &[u8], png: bool) -> Result<Vec<u8>, &'static str> {
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
    transcode_to_png(bytes)
}

#[cfg(target_os = "macos")]
type CF = *const c_void;
#[cfg(target_os = "macos")]
type CFMutable = *mut c_void;

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: CF, bytes: *const u8, length: isize) -> CF;
    fn CFDataCreateMutable(allocator: CF, capacity: isize) -> CFMutable;
    fn CFDataGetBytePtr(data: CF) -> *const u8;
    fn CFDataGetLength(data: CF) -> isize;
    fn CFStringCreateWithBytes(
        alloc: CF,
        bytes: *const u8,
        num_bytes: isize,
        encoding: u32,
        is_external: u8,
    ) -> CF;
    fn CFRelease(value: CF);
    fn CFDictionaryGetValue(dictionary: CF, key: CF) -> CF;
    fn CFNumberGetValue(number: CF, kind: isize, value: *mut c_void) -> bool;
    fn CFGetTypeID(value: CF) -> usize;
    fn CFNumberGetTypeID() -> usize;
}

#[cfg(target_os = "macos")]
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: CF, options: CF) -> CF;
    fn CGImageSourceGetCount(source: CF) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(source: CF, index: usize, options: CF) -> CF;
    fn CGImageSourceCreateImageAtIndex(source: CF, index: usize, options: CF) -> CF;
    fn CGImageDestinationCreateWithData(
        data: CFMutable,
        image_type: CF,
        count: usize,
        options: CF,
    ) -> CFMutable;
    fn CGImageDestinationAddImage(dest: CFMutable, image: CF, properties: CF);
    fn CGImageDestinationFinalize(dest: CFMutable) -> bool;
    static kCGImagePropertyPixelWidth: CF;
    static kCGImagePropertyPixelHeight: CF;
    static kCGImagePropertyDepth: CF;
}

#[cfg(target_os = "macos")]
struct OwnedCF(CF);
#[cfg(target_os = "macos")]
impl OwnedCF {
    fn new(value: CF) -> Result<Self, &'static str> {
        if value.is_null() {
            Err(INVALID)
        } else {
            Ok(Self(value))
        }
    }
}
#[cfg(target_os = "macos")]
impl Drop for OwnedCF {
    fn drop(&mut self) {
        // SAFETY: OwnedCF contains one non-null Create/Copy reference and releases it exactly once.
        unsafe { CFRelease(self.0) }
    }
}

#[cfg(target_os = "macos")]
fn check_dimensions(bytes: &[u8]) -> Result<(), &'static str> {
    // SAFETY: CFDataCreate copies the live input slice. OwnedCF holds each created object through dependent calls.
    unsafe {
        let data = OwnedCF::new(CFDataCreate(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
        ))?;
        let source = OwnedCF::new(CGImageSourceCreateWithData(data.0, std::ptr::null()))?;
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

#[cfg(target_os = "macos")]
pub(super) fn transcode_with_uti(bytes: &[u8], uti: &str) -> Result<Vec<u8>, &'static str> {
    const UTF8_ENCODING: u32 = 0x0800_0100;
    // SAFETY: CFDataCreate copies the live input slice. OwnedCF holds each created object through dependent calls.
    unsafe {
        let data = OwnedCF::new(CFDataCreate(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
        ))?;
        let source = OwnedCF::new(CGImageSourceCreateWithData(data.0, std::ptr::null()))?;
        if CGImageSourceGetCount(source.0) != 1 {
            return Err(INVALID);
        }
        let image = OwnedCF::new(CGImageSourceCreateImageAtIndex(
            source.0,
            0,
            std::ptr::null(),
        ))?;
        let out_data = OwnedCF::new(CFDataCreateMutable(std::ptr::null(), 0).cast_const())?;
        let uti_cf = OwnedCF::new(CFStringCreateWithBytes(
            std::ptr::null(),
            uti.as_ptr(),
            uti.len() as isize,
            UTF8_ENCODING,
            0,
        ))?;
        let dest = OwnedCF::new(
            CGImageDestinationCreateWithData(out_data.0.cast_mut(), uti_cf.0, 1, std::ptr::null())
                .cast_const(),
        )?;
        CGImageDestinationAddImage(dest.0.cast_mut(), image.0, std::ptr::null());
        if !CGImageDestinationFinalize(dest.0.cast_mut()) {
            return Err(INVALID);
        }
        let len = CFDataGetLength(out_data.0);
        if len <= 0 {
            return Err(INVALID);
        }
        if len as usize > MAX_IMAGE_BYTES {
            return Err("粘贴图片超过 20MiB");
        }
        let ptr = CFDataGetBytePtr(out_data.0);
        if ptr.is_null() {
            return Err(INVALID);
        }
        Ok(std::slice::from_raw_parts(ptr, len as usize).to_vec())
    }
}

#[cfg(target_os = "macos")]
fn transcode_to_png(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let png = transcode_with_uti(bytes, "public.png")?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(INVALID);
    }
    Ok(png)
}

#[cfg(not(target_os = "macos"))]
fn check_dimensions(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() < 26 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || &bytes[12..16] != b"IHDR" {
        return Err(INVALID);
    }
    let width = u64::from(u32::from_be_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19],
    ]));
    let height = u64::from(u32::from_be_bytes([
        bytes[20], bytes[21], bytes[22], bytes[23],
    ]));
    let depth = u64::from(bytes[24]);
    if width == 0 || height == 0 || depth == 0 || depth > 16 {
        return Err(INVALID);
    }
    if width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(16))
        .is_none_or(|b| b > PIXEL_BYTES)
    {
        return Err(TOO_LARGE);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn transcode_to_png(_bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    Err(INVALID)
}

#[cfg(test)]
#[path = "clipboard_tests.rs"]
mod ingress_tests;

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
        assert_eq!(normalize(PNG, true).unwrap(), PNG);
        #[cfg(target_os = "macos")]
        {
            let tiff = transcode_with_uti(PNG, "public.tiff").unwrap();
            let converted = normalize(&tiff, false).unwrap();
            assert!(converted.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert_eq!(
                u32::from_be_bytes([converted[16], converted[17], converted[18], converted[19]]),
                1
            );
            assert_eq!(
                u32::from_be_bytes([converted[20], converted[21], converted[22], converted[23]]),
                1
            );
        }
        assert!(normalize(b"not an image", true).is_err());
    }

    #[test]
    fn private_pasteboard_changed_during_conversion_is_rejected() {
        let mut snap = super::ingress_tests::empty_snapshot();
        snap.change_count_before = 7;
        snap.change_count_after = 8;
        snap.image_kind = AgentHubRawImageKind::Png;
        snap.image_declared_bytes = PNG.len();
        snap.image_bytes = PNG.to_vec();
        assert_eq!(
            read_paste(&snap, None).err(),
            Some("剪贴板已变化，请重新粘贴")
        );
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
        let diagnostics = diagnostics_with_hint("fixture diagnostic".into());
        let expected = serde_json::to_string_pretty(&diagnostics).unwrap();
        assert_eq!(format_diagnostics(&diagnostics).unwrap(), expected);
    }

    #[test]
    fn oversized_diagnostic_copy_keeps_private_pasteboard_contents() {
        let diagnostics = diagnostics_with_hint("x".repeat(64 * 1024));
        assert_eq!(format_diagnostics(&diagnostics), Err("诊断超出复制容量"));
    }

    #[test]
    fn oversized_capture_is_rejected_before_image_decoding() {
        let bytes = vec![0; CAPTURE_BYTES + 1];
        assert_eq!(normalize(&bytes, true).unwrap_err(), TOO_LARGE);
    }
}
