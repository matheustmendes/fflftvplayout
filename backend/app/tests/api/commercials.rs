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

#[tokio::test]
#[serial]
async fn week_template_is_saved_per_weekday_with_lengths_read_from_the_files() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let (status, saved) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/week-template/4",
        Some(&admin),
        json!({ "items": [
            { "source": "ad.mp4", "title": "Morning" },
            { "source": "silent.mp4", "in_sec": 0.5 },
        ] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved.as_array().unwrap().len(), 2);
    assert_eq!(saved[0]["title"], "Morning");
    assert_eq!(saved[0]["position"], 0);
    assert_eq!(
        saved[1]["title"], "silent",
        "the title defaults to the file name"
    );
    assert_eq!(saved[1]["position"], 1);
    assert_eq!(saved[1]["in_sec"], 0.5);
    assert!(saved[0]["duration_sec"].as_f64().unwrap() > 0.0);
    assert_eq!(saved[0]["out_sec"], saved[0]["duration_sec"]);

    let (_, other_day) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/week-template/5",
        Some(&admin),
        json!({ "items": [{ "source": "ad.mp4" }] }),
    )
    .await;

    assert_eq!(other_day.as_array().unwrap().len(), 1);

    let (_, thursday) = send(
        &fixture,
        "GET",
        "/api/commercials/1/week-template?weekday=4",
        Some(&operator),
        json!(null),
    )
    .await;
    let (_, week) = send(
        &fixture,
        "GET",
        "/api/commercials/1/week-template",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(thursday.as_array().unwrap().len(), 2);
    assert_eq!(week.as_array().unwrap().len(), 3);

    let (status, replaced) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/week-template/4",
        Some(&admin),
        json!({ "items": [{ "source": "silent.mp4" }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        replaced.as_array().unwrap().len(),
        1,
        "saving replaces the day"
    );
}

#[tokio::test]
#[serial]
async fn week_template_rejects_bad_requests_and_other_roles_and_channels() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;

    for (weekday, body, expected) in [
        (0, json!({ "items": [] }), StatusCode::BAD_REQUEST),
        (8, json!({ "items": [] }), StatusCode::BAD_REQUEST),
        (
            1,
            json!({ "items": [{ "source": "missing.mp4" }] }),
            StatusCode::BAD_REQUEST,
        ),
        (
            1,
            json!({ "items": [{ "source": "../outside.mp4" }] }),
            StatusCode::FORBIDDEN,
        ),
        (
            1,
            json!({ "items": [{ "source": "ad.mp4", "in_sec": 9.0, "out_sec": 2.0 }] }),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (status, answer) = send(
            &fixture,
            "PUT",
            &format!("/api/commercials/1/week-template/{weekday}"),
            Some(&admin),
            body,
        )
        .await;

        assert_eq!(status, expected, "weekday {weekday}: {answer}");
    }

    let (status, _) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/week-template/1",
        Some(&operator),
        json!({ "items": [{ "source": "ad.mp4" }] }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "operators cannot change the template"
    );

    let (status, _) = send(
        &fixture,
        "GET",
        "/api/commercials/2/week-template",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, week) = send(
        &fixture,
        "GET",
        "/api/commercials/1/week-template",
        Some(&admin),
        json!(null),
    )
    .await;

    assert!(
        week.as_array().unwrap().is_empty(),
        "nothing was saved by the rejected calls"
    );
}

/// Upload-converted spot, weekly template, break window: everything a planned
/// day needs, for Thursday 2026-12-10 (inside the campaign of the helper).
async fn prepare_thursday(fixture: &SecurityFixture, admin: &str, operator: &str) -> i64 {
    let advertiser_id = create_advertiser(fixture, admin, "Padaria").await;
    let campaign_id = create_campaign(fixture, admin, advertiser_id).await;
    let (status, spot) = send(
        fixture,
        "POST",
        "/api/commercials/1/spots",
        Some(operator),
        json!({ "campaign_id": campaign_id, "title": "Natal", "file_path": "natal.mxf" }),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED, "{spot}");

    let spot_id = spot["id"].as_i64().unwrap();

    assert_eq!(
        wait_for_spot(fixture, operator, spot_id).await["status"],
        "ready"
    );

    let (status, _) = send(
        fixture,
        "PUT",
        "/api/commercials/1/week-template/4",
        Some(admin),
        json!({ "items": [{ "source": "ad.mp4", "title": "Morning" }, { "source": "silent.mp4" }] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    // The default day starts at 05:59:25, which is where the first break goes.
    let (status, window) = send(
        fixture,
        "POST",
        "/api/commercials/1/break-windows",
        Some(admin),
        json!({ "weekday": 4, "nominal_time": "05:59:25", "duration_sec": 10 }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{window}");

    spot_id
}

#[tokio::test]
#[serial]
async fn a_day_is_planned_from_a_converted_spot_and_written_as_a_playlist() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let spot_id = prepare_thursday(&fixture, &admin, &operator).await;
    let (status, preview) = send(
        &fixture,
        "GET",
        "/api/commercials/1/plan/2026-12-10",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["weekday"], 4);
    assert_eq!(preview["playlist_exists"], false);

    let items = preview["plan"]["items"].as_array().unwrap();
    let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();

    assert_eq!(kinds[0], "spot", "the break opens the day: {kinds:?}");
    assert_eq!(
        items[0]["source"],
        format!("commercials/spot-{spot_id}.mp4")
    );
    assert_eq!(items[0]["start_time"], "05:59:25");
    assert!(
        kinds.contains(&"break_filler"),
        "the break is 10 s, the spot about 3 s"
    );
    assert_eq!(kinds.iter().filter(|k| **k == "program").count(), 2);
    assert_eq!(
        kinds.last(),
        Some(&"day_filler"),
        "the programs are far shorter than a day"
    );
    assert_eq!(
        preview["plan"]["total_sec"],
        preview["plan"]["day_length_sec"]
    );

    let codes: Vec<&str> = preview["plan"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();

    assert!(codes.contains(&"day_filled"));
    assert!(codes.contains(&"break_underfilled"));
    assert!(
        !preview["plan"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["severity"] == "error")
    );

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-10",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "operators only look at the plan"
    );

    let (status, applied) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-10",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{applied}");

    let file = fixture.root.join("playlists/2026/12/2026-12-10.json");
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let program = written["program"].as_array().unwrap();

    assert_eq!(written["date"], "2026-12-10");
    assert_eq!(program.len(), items.len());
    assert_eq!(
        program[0]["source"],
        format!("commercials/spot-{spot_id}.mp4")
    );
    assert_eq!(program[0]["ad"], true, "spots are advertising");
    assert!(
        program
            .iter()
            .any(|m| m["source"] == "ad.mp4" && m.get("ad").is_none())
    );

    let (_, after) = send(
        &fixture,
        "GET",
        "/api/commercials/1/plan/2026-12-10",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(after["playlist_exists"], true);

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-10",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "an existing playlist is not replaced by surprise"
    );

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-10?overwrite=true",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn a_day_without_programs_is_reported_and_never_applied() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let (status, preview) = send(
        &fixture,
        "GET",
        "/api/commercials/1/plan/2026-12-11",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["plan"]["warnings"][0]["code"], "no_programs");
    assert_eq!(preview["plan"]["warnings"][0]["severity"], "error");

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-11?overwrite=true",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        !fixture
            .root
            .join("playlists/2026/12/2026-12-11.json")
            .exists()
    );
}

#[tokio::test]
#[serial]
async fn plan_routes_validate_the_date_and_the_channel() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;

    for date in ["2026-13-01", "tomorrow", "2026-12-1x"] {
        let (status, _) = send(
            &fixture,
            "GET",
            &format!("/api/commercials/1/plan/{date}"),
            Some(&admin),
            json!(null),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST, "{date}");
    }

    for method in ["GET", "POST"] {
        let (status, _) = send(
            &fixture,
            method,
            "/api/commercials/2/plan/2026-12-10",
            Some(&admin),
            json!(null),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN, "{method} on another channel");
    }

    let (status, _) = send(
        &fixture,
        "GET",
        "/api/commercials/1/plan/2026-12-10",
        None,
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn an_applied_plan_promises_its_spots_in_the_airing_log() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let spot_id = prepare_thursday(&fixture, &admin, &operator).await;

    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-10",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    let (status, log) = send(
        &fixture,
        "GET",
        "/api/commercials/1/airlog",
        Some(&operator),
        json!(null),
    )
    .await;
    let rows = log.as_array().unwrap();

    assert_eq!(status, StatusCode::OK, "{log}");
    assert_eq!(rows.len(), 1, "the plan holds one spot: {log}");
    assert_eq!(rows[0]["spot_id"], spot_id);
    assert_eq!(rows[0]["result"], "pending");
    assert_eq!(rows[0]["air_date"], "2026-12-10");
    assert_eq!(rows[0]["advertiser_name"], "Padaria");
    assert!(rows[0]["planned_start"].is_string());
    assert!(rows[0]["actual_start"].is_null());

    // Applying the day again replaces its promises instead of doubling them.
    let (status, _) = send(
        &fixture,
        "POST",
        "/api/commercials/1/plan/2026-12-10?overwrite=true",
        Some(&admin),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK);

    let (_, log) = send(
        &fixture,
        "GET",
        "/api/commercials/1/airlog?date_from=2026-12-10&date_to=2026-12-10&result=pending",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(log.as_array().unwrap().len(), 1);

    for (query, expected) in [
        ("result=played", 0),
        ("kind=live", 0),
        ("date_from=2026-12-11", 0),
        ("limit=1", 1),
    ] {
        let (_, log) = send(
            &fixture,
            "GET",
            &format!("/api/commercials/1/airlog?{query}"),
            Some(&operator),
            json!(null),
        )
        .await;

        assert_eq!(log.as_array().unwrap().len(), expected, "{query}");
    }

    let (status, _) = send(
        &fixture,
        "GET",
        "/api/commercials/1/airlog?date_from=tomorrow",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &fixture,
        "GET",
        "/api/commercials/1/airlog",
        None,
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

async fn send_raw(
    fixture: &SecurityFixture,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = fixture
        .app
        .clone()
        .oneshot(json_request("GET", uri, token, json!(null)))
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();

    (status, headers, bytes.to_vec())
}

/// Log rows as the playout writes them: two spots of one campaign on one day,
/// one aired and one lost to a live broadcast, plus a spot of another campaign.
async fn seed_airlog(fixture: &SecurityFixture, campaign_id: i64, advertiser_id: i64) {
    let pool = &fixture.state.pool;

    for (title, result, planned, actual) in [
        (
            "Natal A",
            "played",
            "2026-12-10T12:00:00.000Z",
            Some("2026-12-10T12:00:01.000Z"),
        ),
        ("Natal B", "live_missed", "2026-12-10T15:00:00.000Z", None),
    ] {
        sqlx::query(
            "INSERT INTO airlog (channel_id, air_date, kind, campaign_id, advertiser_id,
                advertiser_name, campaign_name, spot_title, planned_start, planned_duration_sec,
                actual_start, actual_duration_sec, result)
            VALUES (1, '2026-12-10', 'spot', $1, $2, 'Padaria', 'Natal', $3, $4, 30.0, $5, $6, $7)",
        )
        .bind(campaign_id)
        .bind(advertiser_id)
        .bind(title)
        .bind(planned)
        .bind(actual)
        .bind(actual.map(|_| 29.9))
        .bind(result)
        .execute(pool)
        .await
        .unwrap();
    }

    let other = sqlx::query_scalar::<_, i64>(
        "INSERT INTO advertiser (channel_id, name) VALUES (1, 'Mercado') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let other_campaign = sqlx::query_scalar::<_, i64>(
        "INSERT INTO campaign (channel_id, advertiser_id, name, kind) VALUES (1, $1, 'Verão', 'standalone') RETURNING id",
    )
    .bind(other)
    .fetch_one(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO airlog (channel_id, air_date, kind, campaign_id, advertiser_id,
            advertiser_name, campaign_name, spot_title, planned_start, planned_duration_sec,
            actual_start, actual_duration_sec, result)
        VALUES (1, '2026-12-10', 'spot', $1, $2, 'Mercado', 'Verão', '=cmd()', '2026-12-10T13:00:00.000Z',
            30.0, '2026-12-10T13:00:00.000Z', 30.0, 'played')",
    )
    .bind(other_campaign)
    .bind(other)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
#[serial]
async fn report_settings_are_validated_and_only_administrators_change_them() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let storage = fixture.root.join("storage");

    let (status, defaults) = send(
        &fixture,
        "GET",
        "/api/commercials/1/report-settings",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{defaults}");
    assert_eq!(defaults["station_name"], "");
    assert_eq!(defaults["logo_path"], "");

    std::fs::write(storage.join("notes.txt"), b"x").unwrap();
    std::fs::create_dir_all(storage.join("branding")).unwrap();

    let made = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=200x60",
            "-frames:v",
            "1",
        ])
        .arg(storage.join("branding/logo.png"))
        .status()
        .unwrap();

    assert!(made.success());

    let body = |logo: &str| {
        json!({
            "station_name": "TV Exemplo",
            "station_details": "Rua das Flores, 100\nCNPJ 00.000.000/0001-00",
            "logo_path": logo,
            "footer_note": "Documento sem valor fiscal."
        })
    };

    let (status, _) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/report-settings",
        Some(&operator),
        body(""),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);

    for (logo, why) in [
        ("branding/missing.png", "the file does not exist"),
        ("notes.txt", "it is not an image"),
        ("../outside.png", "it leaves the storage"),
    ] {
        let (status, _) = send(
            &fixture,
            "PUT",
            "/api/commercials/1/report-settings",
            Some(&admin),
            body(logo),
        )
        .await;

        assert!(status.is_client_error(), "{why}: {status}");
    }

    let (status, _) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/report-settings",
        Some(&admin),
        json!({ "station_name": "x".repeat(500) }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, saved) = send(
        &fixture,
        "PUT",
        "/api/commercials/1/report-settings",
        Some(&admin),
        body("branding/logo.png"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{saved}");

    let (_, read) = send(
        &fixture,
        "GET",
        "/api/commercials/1/report-settings",
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(read["station_name"], "TV Exemplo");
    assert_eq!(read["logo_path"], "branding/logo.png");
}

#[tokio::test]
#[serial]
async fn an_operator_downloads_the_report_as_summary_csv_and_pdf() {
    let fixture = commercial_fixture().await;
    let admin = access_token(&fixture.state, CHANNEL_ADMIN_ID).await;
    let operator = access_token(&fixture.state, 1).await;
    let advertiser_id = create_advertiser(&fixture, &admin, "Padaria").await;
    let campaign_id = create_campaign(&fixture, &admin, advertiser_id).await;

    seed_airlog(&fixture, campaign_id, advertiser_id).await;

    let period = "date_from=2026-12-01&date_to=2026-12-31";
    let (status, summary) = send(
        &fixture,
        "GET",
        &format!("/api/commercials/1/reports/summary?{period}&campaign_id={campaign_id}"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["header"]["campaign"], "Natal");
    assert_eq!(summary["header"]["advertiser"], "Padaria");
    assert_eq!(
        summary["lines"].as_array().unwrap().len(),
        2,
        "only this campaign"
    );
    assert_eq!(summary["totals"]["planned"], 2);
    assert_eq!(summary["totals"]["aired"], 1);
    assert_eq!(summary["totals"]["missed"], 1);
    assert_eq!(
        summary["lines"][1]["note"],
        "Transmissão ao vivo no horário"
    );

    let (_, everything) = send(
        &fixture,
        "GET",
        &format!("/api/commercials/1/reports/summary?{period}"),
        Some(&operator),
        json!(null),
    )
    .await;

    assert_eq!(everything["lines"].as_array().unwrap().len(), 3);

    let (status, headers, csv) = send_raw(
        &fixture,
        &format!("/api/commercials/1/reports/export.csv?{period}"),
        Some(&operator),
    )
    .await;
    let text = String::from_utf8(csv.clone()).unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(&csv[..3], [0xEF, 0xBB, 0xBF]);
    assert_eq!(headers["content-type"], "text/csv; charset=utf-8");
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"veiculacao-todos-2026-12-01-2026-12-31.csv\""
    );
    assert!(text.contains("Natal A"));
    assert!(
        text.contains("'=cmd()"),
        "a formula in a name is defused: {text}"
    );

    let (status, headers, pdf) = send_raw(
        &fixture,
        &format!("/api/commercials/1/reports/export.pdf?{period}&campaign_id={campaign_id}"),
        Some(&operator),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(pdf.starts_with(b"%PDF"));
    assert_eq!(headers["content-type"], "application/pdf");
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"veiculacao-natal-2026-12-01-2026-12-31.pdf\""
    );

    for (uri, expected) in [
        (
            "/api/commercials/1/reports/summary?date_from=2026-12-01",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/commercials/1/reports/summary?date_from=x&date_to=y",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/commercials/1/reports/summary?date_from=2026-12-31&date_to=2026-12-01",
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/commercials/1/reports/summary?date_from=2020-01-01&date_to=2026-12-01",
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (status, _, _) = send_raw(&fixture, uri, Some(&operator)).await;

        assert_eq!(status, expected, "{uri}");
    }

    let (status, _, _) = send_raw(
        &fixture,
        &format!("/api/commercials/1/reports/export.csv?{period}&campaign_id=9999"),
        Some(&operator),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = send_raw(
        &fixture,
        &format!("/api/commercials/1/reports/export.pdf?{period}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
