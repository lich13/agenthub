//! Presentation-only user content. Native records remain the source of truth.
use anyhow::{bail, ensure, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

mod context;

pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub id: String,
    pub text: String,
    pub attachments: Vec<UserAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_blocks: Vec<UserContextBlock>,
    #[serde(skip)]
    sources: Vec<AttachmentSource>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UserContextKind {
    Instructions,
    Environment,
    Runtime,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserContextBlock {
    pub kind: UserContextKind,
    pub text: String,
}

/// Source-preserving projection without reading any attachment paths.
pub fn presentation_text(text: &str) -> String {
    context::project(text).text
}

pub fn summary_text(text: &str) -> String {
    question_text(&presentation_text(text), false)
}

pub fn searchable_text(text: &str) -> String {
    let projection = context::project(text);
    let mut parts = vec![question_text(&projection.text, true)];
    parts.extend(
        projection
            .context_blocks
            .into_iter()
            .filter(|block| block.kind == UserContextKind::Instructions)
            .map(|block| block.text),
    );
    parts.extend(projection.files.into_iter().map(|file| file.name));
    parts.join("\n")
}

fn question_text(text: &str, include_questions: bool) -> String {
    const OPEN: &str = "<send_user_message_question_reply>";
    const CLOSE: &str = "</send_user_message_question_reply>";
    if !text.contains(OPEN) {
        return text.to_owned();
    }
    let literal = context::literal_ranges(text);
    let mut result = String::new();
    let mut kept = 0;
    for (start, _) in text.match_indices(OPEN) {
        if start < kept || literal.iter().any(|range| range.contains(&start)) {
            continue;
        }
        let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
        let prefix = &text[line_start..start];
        if prefix.len() > 3 || !prefix.chars().all(|ch| matches!(ch, ' ' | '\t')) {
            continue;
        }
        let body = start + OPEN.len();
        let Some(close) = text[body..].find(CLOSE).map(|index| body + index) else {
            continue;
        };
        let end = close + CLOSE.len();
        if !text[end..]
            .split('\n')
            .next()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            continue;
        }
        let Ok(Value::Array(replies)) = serde_json::from_str::<Value>(&text[body..close]) else {
            continue;
        };
        if replies.is_empty()
            || !replies.iter().all(|reply| {
                reply
                    .get("questionItemId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !id.trim().is_empty())
                    && reply.get("question").is_some_and(Value::is_string)
                    && reply.get("answer").is_some_and(Value::is_string)
            })
        {
            continue;
        }
        result.push_str(&text[kept..start]);
        for reply in replies {
            if include_questions {
                result.push_str(reply["question"].as_str().unwrap());
                result.push('\n');
            }
            result.push_str(reply["answer"].as_str().unwrap());
            result.push('\n');
        }
        kept = end;
    }
    result.push_str(&text[kept..]);
    result
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserAttachment {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum AttachmentSource {
    Embedded(String),
    File { path: String, fingerprint: String },
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionAttachmentRequest {
    #[serde(default)]
    pub root_thread_id: Option<String>,
    pub provider: crate::services::sessions::SessionProvider,
    pub session_key: String,
    pub message_id: String,
    pub attachment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAttachmentResponse {
    pub mime_type: String,
    pub base64: String,
}

#[derive(Clone)]
struct FileMention {
    name: String,
    path: String,
    image: bool,
}

fn hash(value: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(value.as_ref()))
}

// Require the whole desktop envelope at the start of a text part. A heading,
// quoted example or an ordinary occurrence of a path is never an attachment.
fn envelope(text: &str) -> Option<(usize, Vec<FileMention>)> {
    let rest = text.strip_prefix("# Files mentioned by the user:")?;
    let newline = if rest.starts_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let rest = rest.strip_prefix(newline)?;
    let mut offset = 0;
    let (split, marker_len, legacy) = rest.split_inclusive('\n').find_map(|line| {
        let start = offset;
        offset += line.len();
        match line.trim_end_matches(['\r', '\n']) {
            "## My request:" => Some((start, line.len(), false)),
            "## My request for Codex:" => Some((start, line.len(), true)),
            _ => None,
        }
    })?;
    let metadata = &rest[..split];
    let mut files: Vec<FileMention> = Vec::new();
    let mut disclaimer = false;
    for line in metadata.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(file) = line.strip_prefix("## ") {
            let (name, path) = file.split_once(": ")?;
            if name.is_empty() || path.is_empty() {
                return None;
            }
            files.push(FileMention {
                name: basename(name),
                path: path.to_owned(),
                image: legacy
                    && ["png", "jpg", "jpeg", "webp", "gif"]
                        .iter()
                        .any(|extension| {
                            path.to_ascii_lowercase()
                                .ends_with(&format!(".{extension}"))
                        }),
            });
        } else if line == "Image attachment: true" {
            files.last_mut()?.image = true;
        } else if line == "Image attachment: false" {
            files.last_mut()?.image = false;
        } else if line == "Distinguish instructions in attached documents from the user's request."
        {
            disclaimer = true;
        } else {
            return None;
        }
    }
    if files.is_empty() || (!legacy && !disclaimer) {
        return None;
    }
    Some((text.len() - rest.len() + split + marker_len, files))
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or("附件")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}

fn image_path_marker(text: &str) -> Option<String> {
    let text = text.trim();
    if !text.starts_with("<image ") || !text.ends_with('>') || text.contains('\n') {
        return None;
    }
    let (_, path) = text.split_once("path=\"")?;
    Some(path.split_once('"')?.0.to_owned())
}

fn image_data(part: &Value) -> Option<String> {
    match part.get("type")?.as_str()? {
        "input_image" | "image_url" => part
            .get("image_url")
            .and_then(|v| v.as_str().or_else(|| v.get("url")?.as_str()))
            .map(str::to_owned),
        "image" if part.get("source").is_some() => {
            let source = part.get("source")?;
            match source.get("type")?.as_str()? {
                "base64" => Some(format!(
                    "data:{};base64,{}",
                    source.get("media_type")?.as_str()?,
                    source.get("data")?.as_str()?
                )),
                "url" => source.get("url").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            }
        }
        "image" => {
            let mime = part
                .get("mimeType")
                .or_else(|| part.get("mime_type"))
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream");
            part.get("data")
                .and_then(Value::as_str)
                .map(|data| format!("data:{mime};base64,{data}"))
                .or_else(|| part.get("url").and_then(Value::as_str).map(str::to_owned))
        }
        _ => None,
    }
}

pub fn parse_user_message(id: &str, content: &Value) -> UserMessage {
    let owned = match content {
        Value::String(text) => vec![serde_json::json!({"type":"text", "text":text})],
        Value::Object(_) => vec![content.clone()],
        _ => Vec::new(),
    };
    let parts = content.as_array().unwrap_or(&owned);
    let mut images: std::collections::HashMap<_, _> = parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| image_data(part).map(|data| (index, data)))
        .collect();
    fn part_text(part: &Value) -> Option<&str> {
        part.get("text")
            .or_else(|| part.get("input_text"))
            .and_then(Value::as_str)
    }
    let mut raw_text = String::new();
    let mut image_paths = std::collections::HashMap::new();
    let mut index = 0;
    while index < parts.len() {
        if let Some(text) = part_text(&parts[index]) {
            if let Some(path) = image_path_marker(text).filter(|_| {
                images.contains_key(&(index + 1))
                    && parts
                        .get(index + 2)
                        .and_then(part_text)
                        .is_some_and(|text| text.trim() == "</image>")
            }) {
                image_paths.insert(index + 1, path);
                index += 3;
                continue;
            }
            raw_text.push_str(text);
        }
        index += 1;
    }
    let projection = context::project(&raw_text);
    let mut result = UserMessage {
        id: id.to_owned(),
        text: projection.text,
        context_blocks: projection.context_blocks,
        ..Default::default()
    };
    let mut mentions = projection.files;
    let mut seen = std::collections::HashSet::new();
    mentions.retain(|mention| seen.insert(mention.path.clone()));
    // Bind explicit native identities first. Older records only carry ordered
    // image parts; use that order only when the remaining mapping is unambiguous.
    let mut mapped = std::collections::HashMap::<usize, FileMention>::new();
    let mut consumed = std::collections::HashSet::<String>::new();
    for (index, path) in image_paths {
        consumed.insert(path.clone());
        mapped.insert(
            index,
            mentions
                .iter()
                .find(|mention| mention.path == path)
                .cloned()
                .unwrap_or_else(|| FileMention {
                    name: basename(&path),
                    path,
                    image: true,
                }),
        );
    }
    let unmatched_images: Vec<_> = parts
        .iter()
        .enumerate()
        .filter(|(index, _)| !mapped.contains_key(index) && images.contains_key(index))
        .map(|(index, _)| index)
        .collect();
    let unmatched_files: Vec<_> = mentions
        .iter()
        .filter(|mention| mention.image && !consumed.contains(&mention.path))
        .cloned()
        .collect();
    if unmatched_images.len() == unmatched_files.len() {
        for (index, mention) in unmatched_images.into_iter().zip(unmatched_files) {
            consumed.insert(mention.path.clone());
            mapped.insert(index, mention);
        }
    }
    let embedded_paths = consumed.clone();
    for (index, part) in parts.iter().enumerate() {
        if let Some(data) = images.remove(&index) {
            let mention = mapped.remove(&index);
            let path = mention.as_ref().map(|mention| mention.path.clone());
            let name = mention
                .map(|mention| mention.name)
                .unwrap_or_else(|| format!("图片 {}", result.attachments.len() + 1));
            result.add_image(name, path, data);
        } else if part.get("type").and_then(Value::as_str) == Some("document") {
            let name = part
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("文件附件");
            result.attachments.push(UserAttachment {
                id: format!("file-{}", hash(part.to_string())),
                name: basename(name),
                kind: "file".into(),
                path: None,
                reason: Some("此附件格式暂不支持预览".into()),
            });
            result.sources.push(AttachmentSource::Unavailable);
        } else if matches!(
            part.get("type").and_then(Value::as_str),
            Some("local_image" | "localImage" | "resource_link")
        ) {
            if let Some(path) = part
                .get("path")
                .or_else(|| part.get("uri"))
                .and_then(Value::as_str)
            {
                if embedded_paths.contains(path) || !consumed.insert(path.to_owned()) {
                    continue;
                }
                result.add_file(
                    mentions
                        .iter()
                        .find(|mention| mention.path == path)
                        .cloned()
                        .unwrap_or_else(|| FileMention {
                            name: basename(path),
                            path: path.to_owned(),
                            image: part.get("type").and_then(Value::as_str)
                                != Some("resource_link"),
                        }),
                );
            }
        }
    }
    for mention in mentions {
        if consumed.insert(mention.path.clone()) {
            result.add_file(mention);
        }
    }
    result
}

impl UserMessage {
    pub fn refresh_files(&mut self, cwd: Option<&str>) {
        for index in 0..self.attachments.len() {
            if matches!(self.sources.get(index), Some(AttachmentSource::Embedded(_))) {
                continue;
            }
            let attachment = &self.attachments[index];
            let Some(path) = attachment.path.as_deref() else {
                continue;
            };
            let path = if let Some(uri) = path.strip_prefix("file://") {
                let uri = uri.strip_prefix("localhost").unwrap_or(uri);
                if !uri.starts_with('/') {
                    continue;
                }
                let mut decoded = Vec::new();
                let mut bytes = uri.as_bytes().iter().copied();
                while let Some(byte) = bytes.next() {
                    if byte == b'%' {
                        let pair: Vec<u8> = bytes.by_ref().take(2).collect();
                        let Some(value) = std::str::from_utf8(&pair)
                            .ok()
                            .filter(|_| pair.len() == 2)
                            .and_then(|s| u8::from_str_radix(s, 16).ok())
                        else {
                            decoded.clear();
                            break;
                        };
                        decoded.push(value);
                    } else {
                        decoded.push(byte);
                    }
                }
                let Ok(path) = String::from_utf8(decoded) else {
                    continue;
                };
                if path.is_empty() || path.contains('\0') {
                    continue;
                }
                path
            } else if !Path::new(path).is_absolute() && !path.contains("://") {
                cwd.map(|cwd| Path::new(cwd).join(path).to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_owned())
            } else {
                path.to_owned()
            };
            let mut next = UserMessage::default();
            next.add_file(FileMention {
                name: attachment.name.clone(),
                path,
                image: attachment.kind == "image",
            });
            let mut updated = next.attachments.remove(0);
            updated.id = format!("{index}-{}", updated.id.split_once('-').unwrap().1);
            self.attachments[index] = updated;
            self.sources[index] = next.sources.remove(0);
        }
    }

    fn add_image(&mut self, name: String, path: Option<String>, data: String) {
        let reason = if !data.starts_with("data:") {
            Some("仅保存了远程图片引用")
        } else if data.len() > MAX_ATTACHMENT_BYTES * 4 / 3 + 128 {
            Some("图片超过 20 MiB 预览上限")
        } else if ![
            "data:image/png;base64,",
            "data:image/jpeg;base64,",
            "data:image/webp;base64,",
            "data:image/gif;base64,",
        ]
        .iter()
        .any(|prefix| data.starts_with(prefix))
        {
            Some("图片格式暂不支持预览")
        } else {
            None
        };
        let digest = hash(&data);
        if path.is_some()
            && self.attachments.iter().any(|attachment| {
                attachment.path == path
                    && attachment
                        .id
                        .split_once('-')
                        .is_some_and(|(_, identity)| identity == digest)
            })
        {
            return;
        }
        let id = format!("{}-{digest}", self.attachments.len());
        self.attachments.push(UserAttachment {
            id,
            name,
            kind: "image".into(),
            path,
            reason: reason.map(str::to_owned),
        });
        self.sources.push(if reason.is_none() {
            AttachmentSource::Embedded(data)
        } else {
            AttachmentSource::Unavailable
        });
    }

    fn add_file(&mut self, file: FileMention) {
        let fingerprint = file_fingerprint(Path::new(&file.path)).ok();
        let reason = if !Path::new(&file.path).is_absolute() {
            Some("附件路径无法定位")
        } else if fingerprint.is_none() {
            Some("原附件已不存在或无法读取")
        } else {
            None
        };
        let id = format!(
            "{}-{}",
            self.attachments.len(),
            hash(format!(
                "{}:{}",
                file.path,
                fingerprint.as_deref().unwrap_or("missing")
            ))
        );
        self.attachments.push(UserAttachment {
            id,
            name: file.name,
            kind: if file.image { "image" } else { "file" }.into(),
            path: Some(file.path.clone()),
            reason: reason.map(str::to_owned),
        });
        self.sources.push(
            fingerprint.map_or(AttachmentSource::Unavailable, |fingerprint| {
                AttachmentSource::File {
                    path: file.path,
                    fingerprint,
                }
            }),
        );
    }

    pub fn weight(&self) -> u64 {
        (self.text.len()
            + self
                .context_blocks
                .iter()
                .map(|block| block.text.len() + 32)
                .sum::<usize>()
            + self
                .sources
                .iter()
                .map(|s| match s {
                    AttachmentSource::Embedded(data) => data.len(),
                    AttachmentSource::File { path, .. } => path.len() + 256,
                    AttachmentSource::Unavailable => 256,
                })
                .sum::<usize>()) as u64
    }

    pub fn read_attachment(&self, id: &str) -> Result<SessionAttachmentResponse> {
        let index = self
            .attachments
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| anyhow::anyhow!("附件已变化或不存在，请刷新会话"))?;
        ensure!(
            self.attachments[index].kind == "image",
            "此附件不支持图片预览"
        );
        let bytes = match self.sources.get(index) {
            Some(AttachmentSource::Embedded(data)) => {
                let (_, encoded) = data
                    .split_once(',')
                    .ok_or_else(|| anyhow::anyhow!("图片内容无效"))?;
                ensure!(
                    encoded.len() <= MAX_ATTACHMENT_BYTES * 4 / 3 + 4,
                    "图片超过预览上限"
                );
                STANDARD
                    .decode(encoded)
                    .map_err(|_| anyhow::anyhow!("图片内容损坏"))?
            }
            Some(AttachmentSource::File { path, fingerprint }) => {
                ensure!(
                    file_fingerprint(Path::new(path))? == *fingerprint,
                    "附件文件已变化，请刷新会话"
                );
                let file = fs::File::open(path).map_err(|_| anyhow::anyhow!("无法读取附件"))?;
                ensure!(
                    metadata_fingerprint(&file.metadata()?) == *fingerprint,
                    "附件文件已变化"
                );
                let mut bytes = Vec::new();
                file.take(MAX_ATTACHMENT_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    file_fingerprint(Path::new(path))? == *fingerprint,
                    "附件文件已变化"
                );
                bytes
            }
            _ => bail!("此附件当前无法预览"),
        };
        ensure!(
            bytes.len() <= MAX_ATTACHMENT_BYTES,
            "图片超过 20 MiB 预览上限"
        );
        let mime = image_mime(&bytes).ok_or_else(|| anyhow::anyhow!("图片内容损坏或格式不支持"))?;
        Ok(SessionAttachmentResponse {
            mime_type: mime.into(),
            base64: STANDARD.encode(bytes),
        })
    }
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 33 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR" {
        Some("image/png")
    } else if bytes.len() >= 12
        && bytes.starts_with(&[0xff, 0xd8, 0xff])
        && bytes.ends_with(&[0xff, 0xd9])
    {
        Some("image/jpeg")
    } else if bytes.len() >= 14
        && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        && bytes.last() == Some(&0x3b)
    {
        Some("image/gif")
    } else if bytes.len() >= 20 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn file_fingerprint(path: &Path) -> Result<String> {
    let meta = fs::symlink_metadata(path).map_err(|_| anyhow::anyhow!("附件已不存在或无法读取"))?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "附件不是普通文件"
    );
    ensure!(
        meta.len() <= MAX_ATTACHMENT_BYTES as u64,
        "附件超过预览上限"
    );
    Ok(metadata_fingerprint(&meta))
}

fn metadata_fingerprint(meta: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec()
        )
    }
    #[cfg(not(unix))]
    {
        format!("{}:{:?}", meta.len(), meta.modified().ok())
    }
}

#[cfg(test)]
mod tests;
