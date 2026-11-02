use crate::state::AppState;
use agenthub_core::services::system::HostSurface;
use axum::http::{HeaderMap, StatusCode};
use std::net::SocketAddr;

pub struct AuthContext {
    pub admin_id: String,
}

pub fn require_auth(headers: &HeaderMap, state: &AppState) -> Result<AuthContext, StatusCode> {
    let mut values = headers.get_all("x-api-key").iter();
    let key = values
        .next()
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if values.next().is_some()
        || key.len() > 256
        || state.host_surface() != HostSurface::LinuxServerApi
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !state
        .db
        .verify_admin_api_key(key)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(AuthContext {
        admin_id: "api-administrator".into(),
    })
}

pub fn authorize_rpc(
    headers: &HeaderMap,
    state: &AppState,
    peer: Option<SocketAddr>,
) -> Result<(), StatusCode> {
    match require_auth(headers, state) {
        Ok(_) => Ok(()),
        Err(status) => {
            // Do not trust forwarding headers or retain the attempted key. Valid keys
            // bypass this failure limiter so an attacker cannot lock out the admin.
            let peer = peer
                .map(|address| address.ip().to_string())
                .unwrap_or_else(|| "unknown".into());
            if !state
                .auth_limiter
                .lock()
                .expect("auth limiter")
                .check(&peer)
            {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            let _ = state.db.record_audit(
                None,
                "api.auth_denied",
                None,
                None,
                Some(&peer),
                serde_json::json!({}),
            );
            Err(status)
        }
    }
}
