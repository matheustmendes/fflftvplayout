use super::*;

const CHANNEL_ADMIN_ID: i32 = 3;

/// The shared fixture has a regular user (1) and a global administrator (2),
/// both on channel 1. Commercial rights differ by role, so add a channel
/// administrator and copy a playable spot into the storage of channel 1.
async fn commercial_fixture() -> SecurityFixture {
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
    let asset = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage");

    for (source, name) in [
        ("media_mix/ad.mp4", "ad.mp4"),
        ("media_mix/no_audio.mp4", "silent.mp4"),
    ] {
        tokio::fs::copy(asset.join(source), storage.join(name))
            .await
            .unwrap();
    }

    // An MXF like the commercials: MPEG-2 video and one mono track per tone.
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=d=3:s=320x180:r=25",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:r=48000:d=3",
            "-f",
            "lavfi",
            "-i",
            "sine=f=880:r=48000:d=3",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "mpeg2video",
            "-c:a",
            "pcm_s24le",
            "-f",
            "mxf",
        ])
        .arg(storage.join("natal.mxf"))
        .status()
        .unwrap();

    assert!(status.success(), "could not generate the test MXF");

    fixture
}

/// Poll the spot list until the conversion of a spot is no longer running.
async fn wait_for_spot(fixture: &SecurityFixture, token: &str, spot_id: i64) -> serde_json::Value {
    for _ in 0..300 {
        let (_, spots) = send(
            fixture,
            "GET",
            "/api/commercials/1/spots?include_archived=true",
            Some(token),
            json!(null),
        )
        .await;
        let spot = spots
            .as_array()
            .unwrap()
            .iter()
            .find(|spot| spot["id"].as_i64() == Some(spot_id))
            .cloned()
            .unwrap();

        if spot["status"] != "processing" {
            return spot;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    panic!("the conversion of spot {spot_id} did not finish");
}

async fn send(
    fixture: &SecurityFixture,
    method: &str,
    uri: &str,
    token: Option<&str>,
    data: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = fixture
        .app
        .clone()
        .oneshot(json_request(method, uri, token, data))
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);

    (status, body)
}

async fn create_advertiser(fixture: &SecurityFixture, token: &str, name: &str) -> i64 {
    let (status, body) = send(
        fixture,
        "POST",
        "/api/commercials/1/advertisers",
        Some(token),
        json!({ "name": name }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");

    body["id"].as_i64().unwrap()
}

async fn create_campaign(fixture: &SecurityFixture, token: &str, advertiser_id: i64) -> i64 {
    let (status, body) = send(
        fixture,
        "POST",
        "/api/commercials/1/campaigns",
        Some(token),
        json!({
            "advertiser_id": advertiser_id,
            "name": "Natal",
            "start_date": "2026-12-01",
            "end_date": "2026-12-24",
            "max_per_day": 4,
            "position": "first",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");

    body["id"].as_i64().unwrap()
}

#[tokio::test]
#[serial]
async fn requests_without_a_token_are_unauthorized() {
    let fixture = commercial_fixture().await;
    let (status, _) = send(
        &fixture,
        "GET",
        "/api/commercials/1/advertisers",
        None,
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn operators_can_read_but_only_administrators_change_the_catalogue() {
    let fixture = commercial_fixture().await;
    let operator = access_token(&fixture.state, 1).await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    let advertiser_id = create_advertiser(&fixture, &admin, "Padaria Central").await;
    let campaign_id = create_campaign(&fixture, &admin, advertiser_id).await;

    for uri in [
        "/api/commercials/1/advertisers",
        "/api/commercials/1/campaigns",
        "/api/commercials/1/spots",
        "/api/commercials/1/break-windows",
    ] {
        let (status, _) = send(&fixture, "GET", uri, Some(&operator), json!(null)).await;

        assert_eq!(status, StatusCode::OK, "operator must read {uri}");
    }

    let writes = [
        (
            "POST",
            "/api/commercials/1/advertisers".to_string(),
            json!({ "name": "Other" }),
        ),
        (
            "PUT",
            format!("/api/commercials/1/advertisers/{advertiser_id}"),
            json!({ "name": "Renamed" }),
        ),
        (
            "DELETE",
            format!("/api/commercials/1/advertisers/{advertiser_id}"),
            json!(null),
        ),
        (
            "POST",
            "/api/commercials/1/campaigns".to_string(),
            json!({ "advertiser_id": advertiser_id, "name": "X", "kind": "standalone" }),
        ),
        (
            "DELETE",
            format!("/api/commercials/1/campaigns/{campaign_id}"),
            json!(null),
        ),
        (
            "POST",
            "/api/commercials/1/break-windows".to_string(),
            json!({ "weekday": 1, "nominal_time": "20:00:00", "duration_sec": 180 }),
        ),
    ];

    for (method, uri, data) in writes {
        let (status, _) = send(&fixture, method, &uri, Some(&operator), data).await;

        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "operator must not {method} {uri}"
        );
    }

    let (_, advertisers) = send(
        &fixture,
        "GET",
        "/api/commercials/1/advertisers",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(advertisers.as_array().unwrap().len(), 1);
    assert_eq!(advertisers[0]["name"], "Padaria Central");
}

#[tokio::test]
#[serial]
async fn users_cannot_reach_the_commercials_of_another_channel() {
    let fixture = commercial_fixture().await;
    let channel_admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;

    for token in [&channel_admin, &operator] {
        let (status, _) = send(
            &fixture,
            "GET",
            "/api/commercials/2/advertisers",
            Some(token),
            json!(null),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/2/advertisers",
        Some(&channel_admin),
        json!({ "name": "Intruder" }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);

    let global_admin = access_token(&fixture.state, 2).await;
    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/2/advertisers",
        Some(&global_admin),
        json!({ "name": "Allowed" }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn advertiser_names_are_validated_and_unique() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    create_advertiser(&fixture, &admin, "Padaria").await;

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/advertisers",
        Some(&admin),
        json!({ "name": "padaria" }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);

    for body in [
        json!({ "name": "   " }),
        json!({ "name": "Ok", "email": "not-an-email" }),
    ] {
        let (status, _) = send(
            &fixture,
            "POST",
            "/api/commercials/1/advertisers",
            Some(&admin),
            body,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
#[serial]
async fn campaign_lifecycle_validates_updates_and_archives() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let advertiser_id = create_advertiser(&fixture, &admin, "Padaria").await;
    let campaign_id = create_campaign(&fixture, &admin, advertiser_id).await;

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/campaigns",
        Some(&admin),
        json!({ "advertiser_id": advertiser_id, "name": "Backwards",
                "start_date": "2026-12-24", "end_date": "2026-12-01" }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/campaigns",
        Some(&admin),
        json!({ "advertiser_id": 9999, "name": "Orphan",
                "start_date": "2026-12-01", "end_date": "2026-12-24" }),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, updated) = send(
        &fixture,
        "PUT",
        &format!("/api/commercials/1/campaigns/{campaign_id}"),
        Some(&admin),
        json!({ "advertiser_id": advertiser_id, "name": "Natal 2026",
                "start_date": "2026-12-01", "end_date": "2026-12-31",
                "weight": 3, "position": "last" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "Natal 2026");
    assert_eq!(updated["weight"], 3);
    assert_eq!(updated["position"], "last");
    assert_eq!(updated["max_per_day"], serde_json::Value::Null);

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/commercials/1/campaigns/{campaign_id}"),
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, active) = send(
        &fixture,
        "GET",
        "/api/commercials/1/campaigns",
        Some(&admin),
        json!(null),
    )
    .await;
    let (_, all) = send(
        &fixture,
        "GET",
        "/api/commercials/1/campaigns?include_archived=true",
        Some(&admin),
        json!(null),
    )
    .await;

    assert!(active.as_array().unwrap().is_empty());
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(all[0]["active"], false);

    let (status, _) = send(
        &fixture,
        "DELETE",
        "/api/commercials/1/campaigns/9999",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn operator_registers_a_spot_that_is_converted_in_the_background() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let advertiser_id = create_advertiser(&fixture, &admin, "Padaria").await;
    let campaign_id = create_campaign(&fixture, &admin, advertiser_id).await;
    let (status, spot) = send(
        &fixture,
        "POST",
        "/api/commercials/1/spots",
        Some(&operator),
        json!({ "campaign_id": campaign_id, "title": "Natal 30s", "file_path": "natal.mxf" }),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED, "{spot}");
    assert_eq!(spot["status"], "processing");
    assert_eq!(spot["source_path"], "natal.mxf");
    assert_eq!(spot["created_by"], 1);

    let spot_id = spot["id"].as_i64().unwrap();
    let ready = wait_for_spot(&fixture, &operator, spot_id).await;

    assert_eq!(ready["status"], "ready", "{ready}");
    assert_eq!(
        ready["file_path"],
        format!("commercials/spot-{spot_id}.mp4")
    );
    assert_eq!(ready["progress"], 100);
    assert!(ready["duration_sec"].as_f64().unwrap() > 2.0);
    assert!(
        fixture
            .root
            .join(format!("storage/commercials/spot-{spot_id}.mp4"))
            .is_file()
    );

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/spots",
        Some(&operator),
        json!({ "campaign_id": campaign_id, "title": "Again", "file_path": "natal.mxf" }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/commercials/1/spots/{spot_id}"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "operators cannot archive spots"
    );

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/commercials/1/spots/{spot_id}"),
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial]
async fn a_spot_that_cannot_be_converted_reports_why_and_can_be_retried() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let advertiser_id = create_advertiser(&fixture, &admin, "Padaria").await;
    let campaign_id = create_campaign(&fixture, &admin, advertiser_id).await;
    let (_, spot) = send(
        &fixture,
        "POST",
        "/api/commercials/1/spots",
        Some(&operator),
        json!({ "campaign_id": campaign_id, "title": "Mudo", "file_path": "silent.mp4" }),
    )
    .await;
    let spot_id = spot["id"].as_i64().unwrap();
    let failed = wait_for_spot(&fixture, &operator, spot_id).await;

    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"], "the file has no audio track");

    let (status, _) = send(
        &fixture,
        "POST",
        &format!("/api/commercials/1/spots/{spot_id}/retry"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);

    let failed_again = wait_for_spot(&fixture, &operator, spot_id).await;

    assert_eq!(failed_again["status"], "failed");

    let (_, ok_spot) = send(
        &fixture,
        "POST",
        "/api/commercials/1/spots",
        Some(&operator),
        json!({ "campaign_id": campaign_id, "title": "Bom", "file_path": "ad.mp4" }),
    )
    .await;
    let ok_id = ok_spot["id"].as_i64().unwrap();

    assert_eq!(
        wait_for_spot(&fixture, &operator, ok_id).await["status"],
        "ready"
    );

    let (status, _) = send(
        &fixture,
        "POST",
        &format!("/api/commercials/1/spots/{ok_id}/retry"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "a ready spot is not retried");

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/spots/9999/retry",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &fixture,
        "POST",
        &format!("/api/commercials/2/spots/{spot_id}/retry"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "other channels are off limits"
    );
}

#[tokio::test]
#[serial]
async fn conversion_settings_default_validate_and_only_admins_change_them() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let (status, defaults) = send(
        &fixture,
        "GET",
        "/api/commercials/1/settings",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(defaults["audio_mapping"], "auto");
    assert_eq!(defaults["loudness_enabled"], true);
    assert_eq!(defaults["loudness_target_lufs"], -24.0);
    assert_eq!(defaults["loudness_true_peak_dbtp"], -2.0);

    let (status, _) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/settings",
        Some(&operator),
        json!({ "audio_mapping": "mix_all" }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, saved) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/settings",
        Some(&admin),
        json!({ "audio_mapping": "first_two_mono", "loudness_target_lufs": -23.0 }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["audio_mapping"], "first_two_mono");
    assert_eq!(saved["loudness_target_lufs"], -23.0);
    assert_eq!(
        saved["loudness_enabled"], true,
        "omitted fields keep their defaults"
    );

    let (_, stored) = send(
        &fixture,
        "GET",
        "/api/commercials/1/settings",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(stored["audio_mapping"], "first_two_mono");

    for body in [
        json!({ "loudness_target_lufs": 5.0 }),
        json!({ "loudness_true_peak_dbtp": 3.0 }),
    ] {
        let (status, _) = send(
            &fixture,
            "PUT",
            "/api/commercials/1/settings",
            Some(&admin),
            body,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    let (status, _) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/settings",
        Some(&admin),
        json!({ "audio_mapping": "nonsense" }),
    )
    .await;

    assert!(
        status.is_client_error(),
        "unknown mapping rejected, got {status}"
    );
}

#[tokio::test]
#[serial]
async fn spot_files_must_exist_inside_the_channel_storage() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let advertiser_id = create_advertiser(&fixture, &admin, "Padaria").await;
    let campaign_id = create_campaign(&fixture, &admin, advertiser_id).await;

    for (path, expected) in [
        ("missing.mp4", StatusCode::BAD_REQUEST),
        ("../outside.mp4", StatusCode::FORBIDDEN),
        ("/etc/passwd", StatusCode::BAD_REQUEST),
    ] {
        let (status, body) = send(
            &fixture,
            "POST",
            "/api/commercials/1/spots",
            Some(&admin),
            json!({ "campaign_id": campaign_id, "title": "Bad", "file_path": path }),
        )
        .await;

        assert_eq!(status, expected, "{path}: {body}");
    }

    let (_, spots) = send(
        &fixture,
        "GET",
        "/api/commercials/1/spots?include_archived=true",
        Some(&admin),
        json!(null),
    )
    .await;

    assert!(spots.as_array().unwrap().is_empty());
}

#[tokio::test]
#[serial]
async fn break_windows_are_normalized_unique_and_deletable() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let (status, window) = send(
        &fixture,
        "POST",
        "/api/commercials/1/break-windows",
        Some(&admin),
        json!({ "weekday": 3, "nominal_time": " 20:00:00 ", "duration_sec": 180 }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{window}");
    assert_eq!(window["nominal_time"], "20:00:00");
    assert_eq!(window["tolerance_sec"], 60);
    assert_eq!(window["enabled"], true);

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/break-windows",
        Some(&admin),
        json!({ "weekday": 3, "nominal_time": "20:00:00", "duration_sec": 60 }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);

    for body in [
        json!({ "weekday": 8, "nominal_time": "20:00:00", "duration_sec": 60 }),
        json!({ "weekday": 1, "nominal_time": "25:00:00", "duration_sec": 60 }),
        json!({ "weekday": 1, "nominal_time": "20:00:00", "duration_sec": 0 }),
    ] {
        let (status, _) = send(
            &fixture,
            "POST",
            "/api/commercials/1/break-windows",
            Some(&admin),
            body,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    let id = window["id"].as_i64().unwrap();
    let (status, updated) = send(
        &fixture,
        "PUT",
        &format!("/api/commercials/1/break-windows/{id}"),
        Some(&admin),
        json!({ "weekday": 4, "nominal_time": "21:30:00", "duration_sec": 240,
                "tolerance_sec": 30, "enabled": false }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["weekday"], 4);
    assert_eq!(updated["duration_sec"], 240);
    assert_eq!(updated["enabled"], false);

    let (_, monday) = send(
        &fixture,
        "GET",
        "/api/commercials/1/break-windows?weekday=3",
        Some(&admin),
        json!(null),
    )
    .await;

    assert!(monday.as_array().unwrap().is_empty());

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/commercials/1/break-windows/{id}"),
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = send(
        &fixture,
        "DELETE",
        &format!("/api/commercials/1/break-windows/{id}"),
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}
