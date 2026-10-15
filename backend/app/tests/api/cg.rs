use super::*;

const CHANNEL_ADMIN_ID: i32 = 3;

/// A regular user (1) and a global administrator (2) exist on channel 1; add a
/// channel administrator and a small PNG in the channel storage.
async fn cg_fixture() -> SecurityFixture {
    let fixture = security_fixture().await;

    sqlx::query("INSERT INTO auth_user (id, mail, username, password, role_id, two_factor) VALUES (3, 'chadmin@example.org', 'channeladmin', 'unchanged', 2, 0)")
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO auth_user_channels (channel_id, user_id) VALUES (1, 3)")
        .execute(&fixture.state.pool)
        .await
        .unwrap();

    let storage = fixture.root.join("storage");
    tokio::fs::create_dir_all(&storage).await.unwrap();

    let status = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x64:d=1",
            "-frames:v",
            "1",
        ])
        .arg(storage.join("selo.png"))
        .status()
        .unwrap();

    assert!(status.success(), "could not generate the test image");

    fixture
}

fn text_graphic() -> serde_json::Value {
    json!({
        "name": "Disque denúncia",
        "kind": "text",
        "text": "Ligue 181",
        "x_pct": 5,
        "y_pct": 85,
        "size_pct": 5,
        "schedules": [{
            "mode": "clock",
            "weekdays": 31,
            "start_time": "18:00",
            "duration_seconds": 15,
            "repeat_every_seconds": 1800,
            "until_time": "23:00",
        }],
    })
}

#[tokio::test]
#[serial]
async fn operators_read_and_show_but_only_administrators_change() {
    let fixture = cg_fixture().await;
    let operator = access_token(&fixture.state, 1).await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    let (status, created) = send(&fixture, "POST", "/api/cg/1", Some(&admin), text_graphic()).await;

    assert_eq!(status, StatusCode::CREATED, "{created}");

    let id = created["id"].as_i64().unwrap();

    let (status, list) = send(&fixture, "GET", "/api/cg/1", Some(&operator), json!(null)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["schedules"][0]["weekdays"], 31);

    let (status, _) = send(
        &fixture,
        "POST",
        &format!("/api/cg/1/{id}/show"),
        Some(&operator),
        json!({ "seconds": 20 }),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/cg/1/{id}/show"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    for (method, uri, data) in [
        ("POST", "/api/cg/1".to_string(), text_graphic()),
        ("PUT", format!("/api/cg/1/{id}"), text_graphic()),
        ("DELETE", format!("/api/cg/1/{id}"), json!(null)),
    ] {
        let (status, _) = send(&fixture, method, &uri, Some(&operator), data).await;

        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "operator must not {method} {uri}"
        );
    }
}

#[tokio::test]
#[serial]
async fn invalid_graphics_are_refused_with_a_reason() {
    let fixture = cg_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    for (patch, expected) in [
        (json!({ "text": "  " }), "text"),
        (json!({ "x_pct": 120 }), "position"),
        (json!({ "size_pct": 0 }), "size"),
        (json!({ "text_color": "red" }), "#RRGGBB"),
        (
            json!({ "schedules": [{ "mode": "clock", "weekdays": 0, "start_time": "18:00" }] }),
            "day of the week",
        ),
        (
            json!({ "schedules": [{
                "mode": "clock", "start_time": "18:00", "duration_seconds": 60,
                "repeat_every_seconds": 30
            }] }),
            "interval",
        ),
        (
            json!({ "kind": "image", "image_path": "missing.png" }),
            "not found",
        ),
        (
            json!({ "kind": "image", "image_path": "../secret.png" }),
            "storage",
        ),
    ] {
        let mut body = text_graphic();

        for (key, value) in patch.as_object().unwrap() {
            body[key] = value.clone();
        }

        let (status, answer) = send(&fixture, "POST", "/api/cg/1", Some(&admin), body).await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert!(
            answer.to_string().contains(expected),
            "expected '{expected}' in {answer}"
        );
    }
}

#[tokio::test]
#[serial]
async fn an_image_graphic_is_saved_edited_and_deleted() {
    let fixture = cg_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    let (status, created) = send(
        &fixture,
        "POST",
        "/api/cg/1",
        Some(&admin),
        json!({
            "name": "Selo",
            "kind": "image",
            "image_path": "selo.png",
            "x_pct": 80,
            "y_pct": 5,
            "size_pct": 10,
            "schedules": [{
                "mode": "program",
                "program_match": "jornal",
                "offset_seconds": 30,
                "duration_seconds": 20,
            }],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{created}");

    let id = created["id"].as_i64().unwrap();

    assert_eq!(created["schedules"][0]["program_match"], "jornal");

    let (status, edited) = send(
        &fixture,
        "PUT",
        &format!("/api/cg/1/{id}"),
        Some(&admin),
        json!({
            "name": "Selo novo",
            "kind": "image",
            "image_path": "selo.png",
            "x_pct": 10,
            "y_pct": 10,
            "size_pct": 12,
            "schedules": [],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["name"], "Selo novo");
    assert!(edited["schedules"].as_array().unwrap().is_empty());

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/cg/1/{id}"),
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = send(&fixture, "GET", "/api/cg/1", Some(&admin), json!(null)).await;

    assert!(list["items"].as_array().unwrap().is_empty());

    let (status, _) = send(
        &fixture,
        "PUT",
        &format!("/api/cg/1/{id}"),
        Some(&admin),
        text_graphic(),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn a_graphic_can_belong_to_a_campaign_of_the_channel_only() {
    let fixture = cg_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    let (status, advertiser) = send(
        &fixture,
        "POST",
        "/api/commercials/1/advertisers",
        Some(&admin),
        json!({ "name": "Padaria Central" }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{advertiser}");

    let (status, campaign) = send(
        &fixture,
        "POST",
        "/api/commercials/1/campaigns",
        Some(&admin),
        json!({
            "advertiser_id": advertiser["id"],
            "name": "Natal",
            "start_date": "2026-12-01",
            "end_date": "2026-12-24",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{campaign}");

    let mut body = text_graphic();

    body["campaign_id"] = campaign["id"].clone();

    let (status, saved) = send(&fixture, "POST", "/api/cg/1", Some(&admin), body.clone()).await;

    assert_eq!(status, StatusCode::CREATED, "{saved}");
    assert_eq!(saved["campaign_id"], campaign["id"]);

    body["campaign_id"] = json!(9999);

    let (status, answer) = send(&fixture, "POST", "/api/cg/1", Some(&admin), body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert!(answer.to_string().contains("campaign"));
}
