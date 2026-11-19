use super::{await_owner, session_token, DesktopError};
use std::{cell::Cell, time::Duration};

#[tokio::test(start_paused = true)]
async fn await_owner_retries_all_transient_owner_errors() {
    let transient_errors = [
        DesktopError::OwnerUnavailable.user_message().to_owned(),
        DesktopError::Unavailable.user_message().to_owned(),
        DesktopError::Disconnected.user_message().to_owned(),
    ];
    let attempts = Cell::new(0);

    let result = await_owner(|| {
        let attempt = attempts.get();
        attempts.set(attempt + 1);
        let error = transient_errors.get(attempt).cloned();
        async move {
            match error {
                Some(error) => Err(error),
                None => Ok("connected"),
            }
        }
    })
    .await;

    assert_eq!(result, Ok("connected"));
    assert_eq!(attempts.get(), 4);
}

#[tokio::test(start_paused = true)]
async fn await_owner_accepts_success_at_nineteen_and_a_half_seconds() {
    let attempts = Cell::new(0);
    let started = tokio::time::Instant::now();

    let result = await_owner(|| {
        let attempt = attempts.get();
        attempts.set(attempt + 1);
        async move {
            if attempt == 39 {
                Ok("connected")
            } else {
                Err(DesktopError::Unavailable.user_message().to_owned())
            }
        }
    })
    .await;

    assert_eq!(result, Ok("connected"));
    assert_eq!(attempts.get(), 40);
    assert_eq!(
        tokio::time::Instant::now() - started,
        Duration::from_millis(19_500)
    );
}

#[tokio::test(start_paused = true)]
async fn await_owner_stops_at_twenty_seconds() {
    let attempts = Cell::new(0);
    let started = tokio::time::Instant::now();

    let result: Result<(), String> = await_owner(|| {
        attempts.set(attempts.get() + 1);
        async { Err(DesktopError::Disconnected.user_message().to_owned()) }
    })
    .await;

    assert_eq!(
        result,
        Err(DesktopError::OwnerUnavailable.user_message().to_owned())
    );
    assert_eq!(attempts.get(), 40);
    assert_eq!(
        tokio::time::Instant::now() - started,
        Duration::from_secs(20)
    );
}

#[tokio::test(start_paused = true)]
async fn await_owner_does_not_replay_a_fixed_error_or_its_side_effect() {
    let side_effects = Cell::new(0);
    let started = tokio::time::Instant::now();
    let result: Result<(), String> = await_owner(|| {
        side_effects.set(side_effects.get() + 1);
        async { Err("synthetic permanent error".to_owned()) }
    })
    .await;

    assert_eq!(result, Err("synthetic permanent error".to_owned()));
    assert_eq!(side_effects.get(), 1);
    assert_eq!(tokio::time::Instant::now() - started, Duration::ZERO);
}

#[test]
fn session_token_isolated_by_subscription_and_worker_epoch() {
    let current = session_token("subscription-a", "epoch-1");

    assert_eq!(current, "subscription-a.epoch-1");
    assert_ne!(current, session_token("subscription-a", "epoch-2"));
    assert_ne!(current, session_token("subscription-b", "epoch-1"));
}
