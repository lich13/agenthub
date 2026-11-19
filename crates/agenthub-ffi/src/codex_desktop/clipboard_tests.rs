use super::*;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn empty_snapshot() -> RawPasteboardSnapshot {
    RawPasteboardSnapshot {
        change_count_before: 1,
        change_count_after: 1,
        total_item_count: 0,
        file_urls: Vec::new(),
        image_kind: AgentHubRawImageKind::None,
        image_declared_bytes: 0,
        image_bytes: Vec::new(),
        has_string: false,
        string_utf16_len: 0,
        string_text: None,
    }
}

fn file_url(path: &Path) -> String {
    url::Url::from_file_path(path)
        .expect("fixture path is absolute")
        .to_string()
}

fn snapshot_with_file_urls(urls: &[String]) -> RawPasteboardSnapshot {
    let mut snap = empty_snapshot();
    snap.total_item_count = urls.len();
    snap.file_urls = urls
        .iter()
        .map(|u| RawFileUrlItem {
            url: u.clone(),
            utf16_len: u.encode_utf16().count(),
        })
        .collect();
    snap
}

fn snapshot_with_text(text: &str) -> RawPasteboardSnapshot {
    let mut snap = empty_snapshot();
    snap.total_item_count = 1;
    snap.has_string = true;
    snap.string_utf16_len = text.encode_utf16().count();
    if snap.string_utf16_len <= TEXT_BYTES {
        snap.string_text = Some(text.to_owned());
    }
    snap
}

fn selected_paths(content: &PasteContent) -> Vec<PathBuf> {
    match content {
        PasteContent::Files(files) => files
            .iter()
            .map(|file| match file {
                NativeReference::File { path, .. } => path.clone(),
                _ => panic!("clipboard selection must contain file references"),
            })
            .collect(),
        _ => panic!("clipboard content is not a file selection"),
    }
}

fn paste_error(result: Result<PasteContent, &'static str>) -> &'static str {
    match result {
        Ok(_) => panic!("clipboard content unexpectedly succeeded"),
        Err(error) => error,
    }
}

#[test]
fn finder_file_urls_are_read_from_private_items_and_deduplicated() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("中文 名称.txt");
    let second = root.path().join("second.txt");
    fs::write(&first, b"first").unwrap();
    fs::write(&second, b"second").unwrap();

    let first_url = file_url(&first);
    let localhost_first = first_url.replacen("file:///", "file://localhost/", 1);
    let snap = snapshot_with_file_urls(&[first_url, localhost_first, file_url(&second)]);
    let capture = read_paste(&snap, None).unwrap();
    assert_eq!(capture.change_count, 1);
    assert_eq!(
        selected_paths(&capture.content),
        vec![
            first.canonicalize().unwrap(),
            second.canonicalize().unwrap()
        ]
    );
}

#[test]
fn file_urls_support_encoded_names_and_reject_remote_authority_or_directories() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("中文 空格.txt");
    fs::write(&path, b"fixture").unwrap();
    let encoded = file_url(&path);
    assert!(encoded.contains('%'));
    assert_eq!(
        local_file_url(&encoded).and_then(|value| value.canonicalize().ok()),
        Some(path.canonicalize().unwrap())
    );
    assert_eq!(
        local_file_url(&encoded.replacen("file:///", "file://remote.example/", 1)),
        None
    );
    assert_eq!(local_file_url("file://remote.example/tmp/fixture"), None);

    let remote_snap =
        snapshot_with_file_urls(&[encoded.replacen("file:///", "file://remote.example/", 1)]);
    assert_eq!(
        paste_error(read_content(&remote_snap, None)),
        "仅支持本机文件，无法添加此文件地址"
    );

    let dir_snap = snapshot_with_file_urls(&[file_url(root.path())]);
    assert_eq!(
        paste_error(read_content(&dir_snap, None)),
        "仅支持普通文件，不能添加目录"
    );
}

#[test]
fn file_urls_take_priority_over_a_tiff_preview() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("document.txt");
    fs::write(&file, b"document").unwrap();

    let mut snap = snapshot_with_file_urls(&[file_url(&file)]);
    snap.image_kind = AgentHubRawImageKind::Tiff;
    snap.image_declared_bytes = super::tests::PNG.len();
    snap.image_bytes = super::tests::PNG.to_vec();

    let content = read_content(&snap, None).unwrap();
    assert_eq!(selected_paths(&content), vec![file.canonicalize().unwrap()]);
}

#[test]
fn text_paths_accept_absolute_home_and_quoted_lines() {
    let root = tempfile::tempdir().unwrap();
    let absolute = root.path().join("absolute.txt");
    let home_file = root.path().join("中文 空格.txt");
    fs::write(&absolute, b"absolute").unwrap();
    fs::write(&home_file, b"home").unwrap();
    let text = format!(
        "\"{}\"\n~/中文 空格.txt\n{}\n",
        absolute.display(),
        absolute.display()
    );
    let snap = snapshot_with_text(&text);

    let content = read_content(&snap, Some(root.path())).unwrap();
    assert_eq!(
        selected_paths(&content),
        vec![
            absolute.canonicalize().unwrap(),
            home_file.canonicalize().unwrap()
        ]
    );
}

#[test]
fn mixed_or_invalid_text_remains_text_without_consuming_the_valid_prefix() {
    let root = tempfile::tempdir().unwrap();
    let valid = root.path().join("valid.txt");
    fs::write(&valid, b"valid").unwrap();
    for text in [
        format!("{}\nthis is ordinary text", valid.display()),
        format!(
            "{}\n{}",
            valid.display(),
            root.path().join("missing.txt").display()
        ),
        "hello from the ordinary text clipboard".to_owned(),
    ] {
        let snap = snapshot_with_text(&text);
        assert!(matches!(
            read_content(&snap, Some(root.path())).unwrap(),
            PasteContent::Text
        ));
    }
}

#[test]
fn html_is_ignored_and_never_turns_a_remote_value_into_a_request() {
    let mut html_only = empty_snapshot();
    html_only.total_item_count = 1;
    assert!(matches!(
        read_content(&html_only, None).unwrap(),
        PasteContent::Unsupported
    ));

    let text_with_html = snapshot_with_text("ordinary text");
    assert!(matches!(
        read_content(&text_with_html, None).unwrap(),
        PasteContent::Text
    ));
}

#[test]
fn pasteboard_item_and_text_limits_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let mut urls = Vec::new();
    for index in 0..=32 {
        let path = root.path().join(format!("file-{index}.txt"));
        fs::write(&path, b"fixture").unwrap();
        urls.push(file_url(&path));
    }
    let snap = snapshot_with_file_urls(&urls);
    assert_eq!(
        paste_error(read_content(&snap, None)),
        "引用数量超出限制，本次附件未添加"
    );

    let oversized = "x".repeat(TEXT_BYTES + 1);
    let text_snap = snapshot_with_text(&oversized);
    assert!(matches!(
        read_content(&text_snap, None).unwrap(),
        PasteContent::Text
    ));
}

#[test]
fn png_tiff_and_jpeg_are_normalized_to_bounded_png_data() {
    let mut png_snap = empty_snapshot();
    png_snap.total_item_count = 1;
    png_snap.image_kind = AgentHubRawImageKind::Png;
    png_snap.image_declared_bytes = super::tests::PNG.len();
    png_snap.image_bytes = super::tests::PNG.to_vec();
    match read_content(&png_snap, None).unwrap() {
        PasteContent::Image(bytes) => assert_eq!(bytes, super::tests::PNG),
        _ => panic!("PNG should be captured as an image"),
    }

    #[cfg(target_os = "macos")]
    {
        let tiff = transcode_with_uti(super::tests::PNG, "public.tiff").unwrap();
        let mut tiff_snap = empty_snapshot();
        tiff_snap.total_item_count = 1;
        tiff_snap.image_kind = AgentHubRawImageKind::Tiff;
        tiff_snap.image_declared_bytes = tiff.len();
        tiff_snap.image_bytes = tiff;
        let converted_tiff = match read_content(&tiff_snap, None).unwrap() {
            PasteContent::Image(bytes) => bytes,
            _ => panic!("TIFF should be captured as an image"),
        };
        assert!(converted_tiff.starts_with(b"\x89PNG\r\n\x1a\n"));

        let jpeg = transcode_with_uti(super::tests::PNG, "public.jpeg").unwrap();
        let mut jpeg_snap = empty_snapshot();
        jpeg_snap.total_item_count = 1;
        jpeg_snap.image_kind = AgentHubRawImageKind::Jpeg;
        jpeg_snap.image_declared_bytes = jpeg.len();
        jpeg_snap.image_bytes = jpeg;
        let converted_jpeg = match read_content(&jpeg_snap, None).unwrap() {
            PasteContent::Image(bytes) => bytes,
            _ => panic!("JPEG should be captured as an image"),
        };
        assert!(converted_jpeg.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            u32::from_be_bytes([
                converted_jpeg[16],
                converted_jpeg[17],
                converted_jpeg[18],
                converted_jpeg[19]
            ]),
            1
        );
        assert_eq!(
            u32::from_be_bytes([
                converted_jpeg[20],
                converted_jpeg[21],
                converted_jpeg[22],
                converted_jpeg[23]
            ]),
            1
        );
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1_u32;
    let mut b = 0_u32;
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(12 + data.len());
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
    chunk
}

fn oversized_png_fixture() -> Vec<u8> {
    let width = 4097_u32;
    let height = 2048_u32;
    let row_bytes = 1 + width.div_ceil(8) as usize;
    let raw = vec![0_u8; row_bytes * height as usize];

    let mut compressed = vec![0x78, 0x01];
    let mut offset = 0;
    while offset < raw.len() {
        let end = (offset + u16::MAX as usize).min(raw.len());
        let final_block = end == raw.len();
        compressed.push(u8::from(final_block));
        let length = (end - offset) as u16;
        compressed.extend_from_slice(&length.to_le_bytes());
        compressed.extend_from_slice(&(!length).to_le_bytes());
        compressed.extend_from_slice(&raw[offset..end]);
        offset = end;
    }
    compressed.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[1, 0, 0, 0, 0]);

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    png.extend_from_slice(&png_chunk(b"IDAT", &compressed));
    png.extend_from_slice(&png_chunk(b"IEND", &[]));
    png
}

#[test]
fn pixel_budget_rejects_an_image_before_appkit_conversion() {
    let oversized = oversized_png_fixture();
    assert_eq!(normalize(&oversized, true).unwrap_err(), TOO_LARGE);
}

#[test]
fn attachment_paste_disposition_is_optional_for_old_responses() {
    use agenthub_core::services::codex_desktop::{
        DesktopAttachmentResult, DesktopPasteDisposition,
    };
    let old = serde_json::json!({"bindingId": "fixture-binding"});
    let decoded: DesktopAttachmentResult = serde_json::from_value(old).unwrap();
    assert_eq!(decoded.binding_id, "fixture-binding");
    assert!(decoded.paste_disposition.is_none());

    let current = DesktopAttachmentResult {
        binding_id: "fixture-binding".into(),
        paste_disposition: Some(DesktopPasteDisposition::Text),
    };
    let encoded = serde_json::to_value(current).unwrap();
    assert_eq!(encoded["pasteDisposition"], "text");
}
