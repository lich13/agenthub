#![allow(non_snake_case)]

use crate::{
    overview::DesktopState,
    services::jobs::{
        self as job_service, DesktopJobDetailRequest, DesktopJobResponse, DesktopJobsRequest,
    },
};
use anyhow::Result;

#[tauri::command(rename = "jobs.list")]
pub fn listJobs(
    state: tauri::State<'_, DesktopState>,
    limit: Option<u32>,
) -> Result<Vec<DesktopJobResponse>, String> {
    job_service::jobs_with_state(&state, DesktopJobsRequest { limit })
        .map_err(|err| err.to_string())
}

#[tauri::command(rename = "jobs.detail")]
pub fn getJob(
    state: tauri::State<'_, DesktopState>,
    id: String,
    include_output: Option<bool>,
) -> Result<Option<DesktopJobResponse>, String> {
    job_service::job_detail_with_state(&state, DesktopJobDetailRequest { id, include_output })
        .map_err(|err| err.to_string())
}
