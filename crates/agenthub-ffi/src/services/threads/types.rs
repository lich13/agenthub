use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadListRequest {
    pub archived: Option<bool>,
    pub query: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadDetailRequest {
    pub id: String,
    pub limit: Option<usize>,
    pub full: Option<bool>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadBlocksRequest {
    pub id: String,
    pub limit: Option<usize>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopThreadIdRequest {
    #[serde(alias = "threadId", alias = "thread_id")]
    pub thread_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopRenameThreadRequest {
    #[serde(alias = "threadId", alias = "thread_id")]
    pub thread_id: String,
    pub name: String,
}
