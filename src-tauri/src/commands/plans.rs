#![allow(non_snake_case)]

use agenthub_core::services::plans::{PlanSaveRequest, PlanSaveResult};
use std::{fs::OpenOptions, io::Write, path::Path};

#[tauri::command(rename = "plans.save")]
pub async fn savePlanMarkdown(request: PlanSaveRequest) -> Result<PlanSaveResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = dirs::download_dir().ok_or("无法定位下载文件夹")?;
        save_markdown(&directory, request)
    })
    .await
    .map_err(|_| "保存失败，请重试".to_owned())?
}

fn save_markdown(directory: &Path, request: PlanSaveRequest) -> Result<PlanSaveResult, String> {
    let name = &request.filename;
    if name.len() > 240
        || name.starts_with('.')
        || !name.ends_with(".md")
        || name.len() <= 3
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        || request.markdown.trim().is_empty()
        || request.markdown.len() > 20 * 1024 * 1024
    {
        return Err("计划内容或文件名无效".into());
    }
    std::fs::create_dir_all(directory).map_err(|_| "无法访问下载文件夹")?;
    for suffix in 0..1000 {
        let filename = if suffix == 0 {
            name.clone()
        } else {
            format!("{} ({suffix}).md", name.trim_end_matches(".md"))
        };
        let path = directory.join(&filename);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                if file
                    .write_all(request.markdown.as_bytes())
                    .and_then(|_| file.sync_all())
                    .is_err()
                {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err("保存失败，请检查下载文件夹空间和权限".into());
                }
                return Ok(PlanSaveResult { filename });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("无法写入下载文件夹".into()),
        }
    }
    Err("同名文件过多，请清理下载文件夹后重试".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(filename: &str) -> PlanSaveRequest {
        PlanSaveRequest {
            filename: filename.into(),
            markdown: "# 计划\n\n内容\n".into(),
        }
    }

    #[test]
    fn saves_utf8_without_overwriting_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("计划.md"), "existing").unwrap();
        let result = save_markdown(dir.path(), request("计划.md")).unwrap();
        assert_eq!(result.filename, "计划 (1).md");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("计划.md")).unwrap(),
            "existing"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(result.filename)).unwrap(),
            request("计划.md").markdown
        );
    }

    #[test]
    fn rejects_paths_empty_and_oversized_content() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "../escape.md",
            "/tmp/escape.md",
            "nested/file.md",
            "file.txt",
            ".md",
            "a\\b.md",
        ] {
            assert!(save_markdown(dir.path(), request(name)).is_err());
        }
        let mut empty = request("empty.md");
        empty.markdown.clear();
        assert!(save_markdown(dir.path(), empty).is_err());
        let mut large = request("large.md");
        large.markdown = "x".repeat(20 * 1024 * 1024 + 1);
        assert!(save_markdown(dir.path(), large).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
