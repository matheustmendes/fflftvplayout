use super::*;

const CHANNEL_ADMIN_ID: i32 = 3;
const GLOBAL_ADMIN_ID: i32 = 2;
const BOT_TOKEN: &str = "123456789:AAH-abcdefghijklmnopqrstuvwxyz_0123";

/// A regular user (1) and a global administrator (2) exist on channel 1; add a
/// channel administrator.
async fn telegram_fixture() -> SecurityFixture {
    let fixture = security_fixture().await;

    sqlx::query("INSERT INTO auth_user (id, mail, username, password, role_id, two_factor) VALUES (3, 'chadmin@example.org', 'channeladmin', 'unchanged', 2, 0)")
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO auth_user_channels (channel_id, user_id) VALUES (1, 3)")
        .execute(&fixture.state.pool)
        .await
        .unwrap();

    fixture
}

#[tokio::test]
#[serial]
async fn only_administrators_see_and_change_the_bot_settings() {
    let fixture = telegram_fixture().await;
    let operator = access_token(&fixture.state, 1).await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    let (status, _) = send(&fixture, "GET", "/api/telegram/1", None, json!(null)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for (method, uri, body) in [
        ("GET", "/api/telegram/1", json!(null)),
        ("PUT", "/api/telegram/1", json!({ "enabled": false })),
        ("POST", "/api/telegram/1/test", json!(null)),
    ] {
        let (status, _) = send(&fixture, method, uri, Some(&operator), body.clone()).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }

    let (status, view) = send(
        &fixture,
        "GET",
        "/api/telegram/1",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["token_set"], false);
    assert_eq!(view["settings"]["enabled"], false);
    assert_eq!(view["settings"]["alert_output"], true, "alerts start on");
    assert_eq!(view["settings"]["summary_time"], "08:00");
}

#[tokio::test]
#[serial]
async fn the_token_is_saved_by_the_global_administrator_and_never_shown_again() {
    let fixture = telegram_fixture().await;
    let channel_admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let global_admin = access_token(&fixture.state, GLOBAL_ADMIN_ID).await;
    let uri = "/api/telegram/1/token";

    let (status, _) = send(
        &fixture,
        "PUT",
        uri,
        Some(&channel_admin),
        json!({ "token": BOT_TOKEN }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only the global administrator"
    );

    let (status, refused) = send(
        &fixture,
        "PUT",
        uri,
        Some(&global_admin),
        json!({ "token": "isto não é um token" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(refused.to_string().contains("token de bot"), "{refused}");

    let (status, saved) = send(
        &fixture,
        "PUT",
        uri,
        Some(&global_admin),
        json!({ "token": format!("  {BOT_TOKEN} ") }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["token_set"], true);
    assert_eq!(
        handles::select_telegram_token(&fixture.state.pool)
            .await
            .unwrap(),
        BOT_TOKEN,
        "the token is kept without the spaces"
    );

    for (method, uri) in [("GET", "/api/telegram/1"), ("PUT", "/api/telegram/1")] {
        let (_, view) = send(
            &fixture,
            method,
            uri,
            Some(&channel_admin),
            json!({ "enabled": false }),
        )
        .await;

        assert!(!view.to_string().contains("AAH"), "{method}: {view}");
    }

    let (status, removed) = send(
        &fixture,
        "PUT",
        uri,
        Some(&global_admin),
        json!({ "token": "" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(removed["token_set"], false);
}

#[tokio::test]
#[serial]
async fn the_settings_of_a_channel_are_checked_and_kept() {
    let fixture = telegram_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    for (settings, expected) in [
        (json!({ "enabled": true }), "chat"),
        (json!({ "chat_id": "meu grupo" }), "número"),
        (json!({ "summary_time": "25:99" }), "HH:MM"),
    ] {
        let (status, error) =
            send(&fixture, "PUT", "/api/telegram/1", Some(&admin), settings).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert!(error.to_string().contains(expected), "{expected}: {error}");
    }

    let (status, saved) = send(
        &fixture,
        "PUT",
        "/api/telegram/1",
        Some(&admin),
        json!({
            "enabled": true,
            "chat_id": " -1001234567890 ",
            "alert_disk": false,
            "summary_time": "07:30",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["settings"]["chat_id"], "-1001234567890");
    assert_eq!(saved["settings"]["alert_disk"], false);
    assert_eq!(
        saved["settings"]["alert_output"], true,
        "what is not said stays on"
    );
    assert_eq!(saved["settings"]["summary_time"], "07:30");

    let (_, view) = send(
        &fixture,
        "GET",
        "/api/telegram/1",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(view["settings"]["enabled"], true);

    let enabled = handles::select_enabled_telegram_settings(&fixture.state.pool)
        .await
        .unwrap();

    assert_eq!(enabled.len(), 1);
    assert_eq!(enabled[0].chat(), Some(-1001234567890));

    send(
        &fixture,
        "PUT",
        "/api/telegram/1",
        Some(&admin),
        json!({ "enabled": false, "chat_id": "-1001234567890" }),
    )
    .await;

    assert!(
        handles::select_enabled_telegram_settings(&fixture.state.pool)
            .await
            .unwrap()
            .is_empty(),
        "a channel that turned the bot off is not served"
    );
}

#[tokio::test]
#[serial]
async fn the_test_message_says_what_is_missing_before_calling_telegram() {
    let fixture = telegram_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    let (status, error) = send(
        &fixture,
        "POST",
        "/api/telegram/1/test",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.to_string().contains("token"), "{error}");

    handles::update_telegram_token(&fixture.state.pool, BOT_TOKEN)
        .await
        .unwrap();

    let (status, error) = send(
        &fixture,
        "POST",
        "/api/telegram/1/test",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error.to_string().contains("/id"), "{error}");
}
