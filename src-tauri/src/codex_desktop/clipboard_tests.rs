use super::*;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardItem,
    NSPasteboardTypeFileURL, NSPasteboardTypeHTML, NSPasteboardTypePNG, NSPasteboardTypeString,
    NSPasteboardTypeTIFF,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use std::fs;
use std::path::{Path, PathBuf};

fn private_board() -> objc2::rc::Retained<NSPasteboard> {
    let board = NSPasteboard::pasteboardWithUniqueName();
    board.clearContents();
    board
}

fn file_url(path: &Path) -> String {
    tauri::Url::from_file_path(path)
        .expect("fixture path is absolute")
        .to_string()
}

fn write_file_items(board: &NSPasteboard, urls: &[String]) {
    let objects = urls
        .iter()
        .map(|url| {
            let item = NSPasteboardItem::new();
            assert!(item
                .setString_forType(&NSString::from_str(url), unsafe { NSPasteboardTypeFileURL }));
            ProtocolObject::from_retained(item)
        })
        .collect::<Vec<_>>();
    let objects = NSArray::from_retained_slice(&objects);
    assert!(board.writeObjects(&objects));
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

fn tiff_fixture() -> Vec<u8> {
    let bitmap = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(super::tests::PNG))
        .expect("PNG fixture decodes");
    // SAFETY: the empty property dictionary has the documented key/object types.
    unsafe {
        bitmap.representationUsingType_properties(
            NSBitmapImageFileType::TIFF,
            &NSDictionary::<objc2_foundation::NSString, AnyObject>::new(),
        )
    }
    .expect("TIFF fixture encodes")
    .to_vec()
}

fn jpeg_fixture() -> Vec<u8> {
    let bitmap = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(super::tests::PNG))
        .expect("PNG fixture decodes");
    // SAFETY: the empty property dictionary has the documented key/object types.
    unsafe {
        bitmap.representationUsingType_properties(
            NSBitmapImageFileType::JPEG,
            &NSDictionary::<objc2_foundation::NSString, AnyObject>::new(),
        )
    }
    .expect("JPEG fixture encodes")
    .to_vec()
}

#[test]
fn finder_file_urls_are_read_from_private_items_and_deduplicated() {
    autoreleasepool(|_| {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("中文 名称.txt");
        let second = root.path().join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();

        let first_url = file_url(&first);
        let localhost_first = first_url.replacen("file:///", "file://localhost/", 1);
        let board = private_board();
        write_file_items(&board, &[first_url, localhost_first, file_url(&second)]);
        let capture = read_paste(&board, None).unwrap();
        assert_eq!(capture.change_count, board.changeCount());
        assert_eq!(
            selected_paths(&capture.content),
            vec![
                first.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ]
        );
    });
}

#[test]
fn file_urls_support_encoded_names_and_reject_remote_authority_or_directories() {
    autoreleasepool(|_| {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("中文 空格.txt");
        fs::write(&path, b"fixture").unwrap();
        let encoded = file_url(&path);
        assert!(
            encoded.contains("%"),
            "fixture URL should exercise URL encoding"
        );
        assert_eq!(
            local_file_url(&encoded).and_then(|value| value.canonicalize().ok()),
            Some(path.canonicalize().unwrap())
        );
        assert_eq!(
            local_file_url(&encoded.replacen("file:///", "file://remote.example/", 1)),
            None
        );
        assert_eq!(local_file_url("file://remote.example/tmp/fixture"), None);

        let board = private_board();
        write_file_items(
            &board,
            &[encoded.replacen("file:///", "file://remote.example/", 1)],
        );
        assert_eq!(
            paste_error(read_content(&board, None)),
            "仅支持本机文件，无法添加此文件地址"
        );

        let directory_board = private_board();
        write_file_items(&directory_board, &[file_url(root.path())]);
        assert_eq!(
            paste_error(read_content(&directory_board, None)),
            "仅支持普通文件，不能添加目录"
        );
    });
}

#[test]
fn file_urls_take_priority_over_a_tiff_preview() {
    autoreleasepool(|_| {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("document.txt");
        fs::write(&file, b"document").unwrap();
        let item = NSPasteboardItem::new();
        assert!(
            item.setString_forType(&NSString::from_str(&file_url(&file)), unsafe {
                NSPasteboardTypeFileURL
            })
        );
        let tiff = NSData::with_bytes(&tiff_fixture());
        assert!(item.setData_forType(&tiff, unsafe { NSPasteboardTypeTIFF }));
        let object = ProtocolObject::from_retained(item);
        let objects = NSArray::from_retained_slice(&[object]);
        let board = private_board();
        assert!(board.writeObjects(&objects));

        let content = read_content(&board, None).unwrap();
        assert_eq!(selected_paths(&content), vec![file.canonicalize().unwrap()]);
    });
}

#[test]
fn text_paths_accept_absolute_home_and_quoted_lines() {
    autoreleasepool(|_| {
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
        let board = private_board();
        assert!(board.setString_forType(&NSString::from_str(&text), unsafe {
            NSPasteboardTypeString
        }));

        let content = read_content(&board, Some(root.path())).unwrap();
        assert_eq!(
            selected_paths(&content),
            vec![
                absolute.canonicalize().unwrap(),
                home_file.canonicalize().unwrap()
            ]
        );
    });
}

#[test]
fn mixed_or_invalid_text_remains_text_without_consuming_the_valid_prefix() {
    autoreleasepool(|_| {
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
            let board = private_board();
            assert!(board.setString_forType(&NSString::from_str(&text), unsafe {
                NSPasteboardTypeString
            }));
            assert!(matches!(
                read_content(&board, Some(root.path())).unwrap(),
                PasteContent::Text
            ));
            // The browser still owns the original text payload, including its valid prefix.
            assert_eq!(
                unsafe { board.stringForType(NSPasteboardTypeString) }
                    .unwrap()
                    .to_string(),
                text
            );
        }
    });
}

#[test]
fn html_is_ignored_and_never_turns_a_remote_value_into_a_request() {
    autoreleasepool(|_| {
        let board = private_board();
        assert!(board.setString_forType(
            &NSString::from_str("https://remote.example.invalid/private"),
            unsafe { NSPasteboardTypeHTML }
        ));
        assert!(matches!(
            read_content(&board, None).unwrap(),
            PasteContent::Unsupported
        ));

        let board = private_board();
        assert!(
            board.setString_forType(&NSString::from_str("ordinary text"), unsafe {
                NSPasteboardTypeString
            })
        );
        assert!(board.setString_forType(
            &NSString::from_str("<img src=\"https://remote.example.invalid/x\">"),
            unsafe { NSPasteboardTypeHTML }
        ));
        assert!(matches!(
            read_content(&board, None).unwrap(),
            PasteContent::Text
        ));
    });
}

#[test]
fn pasteboard_item_and_text_limits_are_bounded() {
    autoreleasepool(|_| {
        let root = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        let mut urls = Vec::new();
        for index in 0..=32 {
            let path = root.path().join(format!("file-{index}.txt"));
            fs::write(&path, b"fixture").unwrap();
            files.push(path);
            urls.push(file_url(files.last().unwrap()));
        }
        let board = private_board();
        write_file_items(&board, &urls);
        assert_eq!(
            paste_error(read_content(&board, None)),
            "引用数量超出限制，本次附件未添加"
        );

        let board = private_board();
        let oversized = "x".repeat(TEXT_BYTES + 1);
        assert!(
            board.setString_forType(&NSString::from_str(&oversized), unsafe {
                NSPasteboardTypeString
            })
        );
        assert!(matches!(
            read_content(&board, None).unwrap(),
            PasteContent::Text
        ));
    });
}

#[test]
fn png_tiff_and_jpeg_are_normalized_to_bounded_png_data() {
    autoreleasepool(|_| {
        let board = private_board();
        assert!(unsafe {
            board.setData_forType(
                Some(&NSData::with_bytes(super::tests::PNG)),
                NSPasteboardTypePNG,
            )
        });
        match read_content(&board, None).unwrap() {
            PasteContent::Image(bytes) => assert_eq!(bytes, super::tests::PNG),
            _ => panic!("PNG should be captured as an image"),
        }

        let board = private_board();
        let tiff = tiff_fixture();
        assert!(unsafe {
            board.setData_forType(Some(&NSData::with_bytes(&tiff)), NSPasteboardTypeTIFF)
        });
        let converted_tiff = match read_content(&board, None).unwrap() {
            PasteContent::Image(bytes) => bytes,
            _ => panic!("TIFF should be captured as an image"),
        };
        assert!(converted_tiff.starts_with(b"\x89PNG\r\n\x1a\n"));

        let board = private_board();
        let jpeg = jpeg_fixture();
        let jpeg_type = NSString::from_str("public.jpeg");
        assert!(board.setData_forType(Some(&NSData::with_bytes(&jpeg)), &jpeg_type));
        let converted_jpeg = match read_content(&board, None).unwrap() {
            PasteContent::Image(bytes) => bytes,
            _ => panic!("JPEG should be captured as an image"),
        };
        assert!(converted_jpeg.starts_with(b"\x89PNG\r\n\x1a\n"));
        let bitmap = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(&converted_jpeg))
            .expect("normalized image decodes");
        assert_eq!((bitmap.pixelsWide(), bitmap.pixelsHigh()), (1, 1));
    });
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
    // A valid 1-bit grayscale PNG whose dimensions exceed the 128 MiB
    // decoded-surface budget. Stored DEFLATE blocks keep the fixture local and
    // deterministic without adding an image or compression dependency.
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
