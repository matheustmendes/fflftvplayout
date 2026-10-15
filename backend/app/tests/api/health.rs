use super::*;

const CHANNEL_ADMIN_ID: i32 = 3;

async fn health_fixture() -> SecurityFixture {
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
async fn the_dashboard_health_needs_a_login_and_reports_the_filler() {
    let fixture = health_fixture().await;
    let operator = access_token(&fixture.state, 1).await;

    let (status, _) = send(&fixture, "GET", "/api/health/1", None, json!(null)).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, health) = send(
        &fixture,
        "GET",
        "/api/health/1",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{health}");
    assert_eq!(
        health["filler"]["ok"], false,
        "the test channel has no filler file"
    );
    assert_eq!(health["relay"]["configured"], false);
}

#[tokio::test]
#[serial]
async fn only_administrators_set_the_relay_and_a_dead_relay_is_unknown() {
    let fixture = health_fixture().await;
    let operator = access_token(&fixture.state, 1).await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let body = json!({
        "api_url": "http://127.0.0.1:1",
        "path": "canal1",
        "destinations": [{ "name": "TV Praia", "protocol": "rtmp", "match_user": "praia" }],
    });

    for (method, data) in [("GET", json!(null)), ("PUT", body.clone())] {
        let (status, _) = send(&fixture, method, "/api/relay/1", Some(&operator), data).await;

        assert_eq!(status, StatusCode::FORBIDDEN, "operator must not {method}");
    }

    let (status, saved) = send(&fixture, "PUT", "/api/relay/1", Some(&admin), body).await;

    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["destinations"][0]["name"], "TV Praia");

    let (_, health) = send(
        &fixture,
        "GET",
        "/api/health/1",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(health["relay"]["configured"], true);
    assert_eq!(health["relay"]["reachable"], false);
    assert_eq!(health["relay"]["destinations"][0]["state"], "unknown");
    assert!(
        !health.to_string().contains("pass"),
        "no password may reach the screen"
    );
}

#[tokio::test]
#[serial]
async fn an_invalid_relay_setup_is_refused_with_a_reason() {
    let fixture = health_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    for (body, expected) in [
        (json!({ "api_url": "ftp://x", "path": "c" }), "http"),
        (json!({ "api_url": "http://x", "path": "" }), "channel"),
        (
            json!({ "api_url": "http://x", "path": "c",
                "destinations": [{ "name": "A", "protocol": "rtmp" }] }),
            "user or the address",
        ),
        (
            json!({ "api_url": "http://x", "path": "c",
                "destinations": [{ "name": "A", "protocol": "rtmp", "match_user": "a b;" }] }),
            "letters",
        ),
    ] {
        let (status, answer) = send(&fixture, "PUT", "/api/relay/1", Some(&admin), body).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert!(answer.to_string().contains(expected), "{answer}");
    }
}
