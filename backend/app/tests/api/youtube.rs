use super::*;

const CHANNEL_ADMIN_ID: i32 = 3;
const GLOBAL_ADMIN_ID: i32 = 2;

/// A regular user (1) and a global administrator (2) exist on channel 1; add a
/// channel administrator.
async fn youtube_fixture() -> SecurityFixture {
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
async fn only_administrators_use_the_youtube_import() {
    let fixture = youtube_fixture().await;
    let operator = access_token(&fixture.state, 1).await;

    let (status, _) = send(&fixture, "GET", "/api/youtube/1", None, json!(null)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for (method, uri, body) in [
        ("GET", "/api/youtube/1", json!(null)),
        ("GET", "/api/youtube/1/jobs", json!(null)),
        (
            "POST",
            "/api/youtube/1/preview",
            json!({ "url": "https://youtu.be/abc" }),
        ),
        (
            "POST",
            "/api/youtube/1/imports",
            json!({ "url": "https://youtu.be/abc" }),
        ),
        ("DELETE", "/api/youtube/1/imports/1", json!(null)),
        (
            "POST",
            "/api/youtube/1/sources",
            json!({ "url": "https://youtu.be/abc" }),
        ),
        ("DELETE", "/api/youtube/1/sources/1", json!(null)),
    ] {
        let (status, _) = send(&fixture, method, uri, Some(&operator), body.clone()).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }
}

#[tokio::test]
#[serial]
async fn a_channel_administrator_sees_the_screen_but_cannot_change_the_allowed_channels() {
    let fixture = youtube_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    sqlx::query("INSERT INTO youtube_source (id, channel_id, youtube_channel_id, name) VALUES (7, 1, 'UC9jfIeNHlejCauwxqZKOz-w', 'LFTV Oficial')")
        .execute(&fixture.state.pool)
        .await
        .unwrap();

    let (status, view) = send(&fixture, "GET", "/api/youtube/1", Some(&admin), json!(null)).await;

    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["sources"][0]["name"], "LFTV Oficial");
    assert_eq!(view["jobs"], json!([]));

    for (method, uri, body) in [
        (
            "POST",
            "/api/youtube/1/sources",
            json!({ "url": "https://youtu.be/abc" }),
        ),
        ("DELETE", "/api/youtube/1/sources/7", json!(null)),
    ] {
        let (status, _) = send(&fixture, method, uri, Some(&admin), body.clone()).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
    }
}

#[tokio::test]
#[serial]
async fn addresses_that_are_not_youtube_never_reach_the_downloader() {
    let fixture = youtube_fixture().await;
    let admin = access_token(&fixture.state, GLOBAL_ADMIN_ID).await;

    for (uri, body) in [
        (
            "/api/youtube/1/sources",
            json!({ "url": "https://example.com/video" }),
        ),
        (
            "/api/youtube/1/preview",
            json!({ "url": "file:///etc/passwd" }),
        ),
        (
            "/api/youtube/1/imports",
            json!({ "url": "https://youtube.com.evil.example/watch?v=1" }),
        ),
        (
            "/api/youtube/1/imports",
            json!({ "url": "https://youtu.be/abc", "height": 999 }),
        ),
        (
            "/api/youtube/1/imports",
            json!({ "url": "https://youtu.be/abc", "folder": "../fora" }),
        ),
    ] {
        let (status, message) = send(&fixture, "POST", uri, Some(&admin), body.clone()).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} {body}: {message}");
    }
}

#[tokio::test]
#[serial]
async fn the_global_administrator_removes_an_allowed_channel() {
    let fixture = youtube_fixture().await;
    let admin = access_token(&fixture.state, GLOBAL_ADMIN_ID).await;

    sqlx::query("INSERT INTO youtube_source (id, channel_id, youtube_channel_id, name) VALUES (7, 1, 'UC9jfIeNHlejCauwxqZKOz-w', 'LFTV Oficial')")
        .execute(&fixture.state.pool)
        .await
        .unwrap();

    let (status, sources) = send(
        &fixture,
        "DELETE",
        "/api/youtube/1/sources/7",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{sources}");
    assert_eq!(sources, json!([]));

    let (status, _) = send(
        &fixture,
        "DELETE",
        "/api/youtube/1/sources/7",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &fixture,
        "DELETE",
        "/api/youtube/1/imports/999",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
