mod tests {
    use axum::{body::Body, body::to_bytes, extract::Request, http::StatusCode};
    use bytes::Bytes;
    use email_serv::database::Database;
    use email_serv::http::{ApiContext, create_router};
    use http_body_util::Full;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;
    use tower::ServiceExt; // for `call`, `oneshot`, and `ready`

    const TEST_BLAKE3_KEY: &str =
        "327da54c32bbda6f1b56c2e248620d31324b6bf5664fc31ab4d455f38787a5fa";

    async fn create_test_context() -> ApiContext {
        let config = create_test_config();
        let db = Database::new(":memory:")
            .await
            .expect("Failed to create in-memory database");
        db.run_migrations().await.expect("Failed to run migrations");

        ApiContext {
            db,
            blake3_key: test_blake3_key(),
            site_url: "http://localhost:3000".to_string(),
            admin_api_key: "test-admin-key".to_string(),
            email_service: EmailService::new(&config).unwrap(),
        }
    }

    fn test_blake3_key() -> [u8; 32] {
        let mut array = [0u8; 32];
        hex::decode_to_slice(TEST_BLAKE3_KEY, &mut array)
            .expect("Failed to decode test Blake3 key");
        array
    }

    #[tokio::test]
    async fn test_fallback() {
        let app = create_router(create_test_context().await);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/gibberish")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health_check")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_unsubscribe() {
        let email = "test@email.com";
        let hash_bytes = test_blake3_hash(email);
        let hash_hex = hex::encode(hash_bytes);
        let context = create_test_context().await;
        context
            .db
            .insert_subscription(&hash_bytes, email)
            .await
            .unwrap();
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .header("X-Forwarded-For", "127.0.0.1")
                    .uri(format!("/api/unsubscribe?token={}", hash_hex))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_verify() {
        let email = "test@email.com";
        let hash_bytes = test_blake3_hash(email);
        let hash_hex = hex::encode(hash_bytes);
        let context = create_test_context().await;
        context
            .db
            .insert_subscription(&hash_bytes, email)
            .await
            .unwrap();
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .header("X-Forwarded-For", "127.0.0.1")
                    .uri(format!("/api/verify?token={}", hash_hex))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_subscribe() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(r#"{"email": "test@email.de"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_duplicate_subscription() {
        let app = create_router(create_test_context().await);
        let email_json = r#"{"email": "duplicate@email.com"}"#;

        // First subscription
        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(email_json))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::OK);

        // Second subscription (same email)
        let response2 = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(email_json))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_length() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .header(
                        http::header::CONTENT_LENGTH,
                        http::HeaderValue::from_static("10000000"),
                    )
                    .body(Full::<Bytes>::default())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    use email_serv::config::Config;
    use email_serv::email::EmailService;

    fn create_test_config() -> Config {
        Config {
            db_conn: ":memory:".to_string(),
            port: 3000,
            blake3_key: TEST_BLAKE3_KEY.to_string(),
            log_dir: "logs".to_string(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_user: "user".to_string(),
            smtp_pass: "pass".to_string(),
            email_from: "test@example.com".to_string(),
            admin_api_key: "secret".to_string(),
            site_url: "http://localhost:3000".to_string(),
        }
    }

    #[tokio::test]
    async fn test_subscribe_idempotency_and_outbox() {
        let context = create_test_context().await;
        let db = context.db.clone();
        let app = create_router(context);
        let email = "idempotent@test.com";
        let body = serde_json::json!({ "email": email });

        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::OK);

        let pending = db.get_pending_outbox_items().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].recipient_email, email);
        assert_eq!(pending[0].email_type, "verification");

        let response2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::OK);
        let pending_after = db.get_pending_outbox_items().await.unwrap();
        assert_eq!(pending_after.len(), 1);

        let hash = test_blake3_hash(email);
        db.verify_subscription(&hash).await.unwrap();

        let response3 = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response3.status(), StatusCode::OK);
        let pending_final = db.get_pending_outbox_items().await.unwrap();
        assert_eq!(pending_final.len(), 1);
    }

    #[tokio::test]
    async fn test_generate_verification_link() {
        let site_url = "http://localhost:3000";
        let blake3_key = test_blake3_key();

        let test_emails = vec![
            "user1@test.com",
            "another.user@domain.co.uk",
            "simple@test.de",
        ];

        for email in test_emails {
            let link = email_serv::http::subscription::generate_verification_link(
                email,
                &blake3_key,
                site_url,
            );

            assert!(link.starts_with(site_url));
            assert!(link.contains("/api/verify?token="));

            let token = link.split("token=").last().unwrap();
            let expected_hash = test_blake3_hash(email);
            assert_eq!(token, hex::encode(expected_hash));
        }
    }

    #[tokio::test]
    async fn test_render_verification_template() {
        let config = create_test_config();
        let email_service = EmailService::new(&config).unwrap();
        let subscriber_email = "test@example.com";
        let verification_url = "http://localhost:3000/api/verify?token=123456";
        let site_url = "http://localhost:3000";

        let context = serde_json::json!({
            "subscriber_email": subscriber_email,
            "verification_url": verification_url,
            "site_url": site_url
        });

        let rendered = email_service
            .render_template("welcome/verification_body.html", &context)
            .unwrap();

        assert!(rendered.contains("Verify Email Address"));
        assert!(rendered.contains("http://localhost:3000/api/verify?token=123456"));
    }

    #[tokio::test]
    async fn test_render_welcome_template() {
        let config = create_test_config();
        let email_service = EmailService::new(&config).unwrap();
        let subscriber_email = "test@example.com";
        let unsubscribe_url = "http://example.com/unsubscribe/123";
        let site_url = "http://localhost:3000";

        let context = serde_json::json!({
            "subscriber_email": subscriber_email,
            "unsubscribe_url": unsubscribe_url,
            "site_url": site_url
        });

        let rendered = email_service
            .render_template("welcome/body.html", &context)
            .unwrap();

        assert!(rendered.contains("Welcome, test@example.com!"));
        assert!(rendered.contains("http://example.com/unsubscribe/123"));
        assert!(rendered.contains("Email-Serv. All rights reserved."));
    }

    #[tokio::test]
    async fn test_render_newsletter_template() {
        let config = create_test_config();
        let email_service = EmailService::new(&config).unwrap();
        let context = serde_json::json!({
            "subscriber_email": "test@example.com",
            "unsubscribe_url": "http://example.com/unsub",
            "site_url": "http://localhost:3000"
        });

        let rendered = email_service
            .render_template("newsletters/1/body.html", &context)
            .unwrap();

        assert!(rendered.contains("Newsletter Edition #1"));
        assert!(rendered.contains("http://example.com/unsub"));
    }

    #[tokio::test]
    async fn test_get_all_verified_subscribers() {
        let context = create_test_context().await;
        let db = context.db;

        // Insert some subscribers
        let email1 = "verified1@test.com";
        let hash1 = test_blake3_hash(email1);
        db.insert_subscription(&hash1, email1).await.unwrap();
        db.verify_subscription(&hash1).await.unwrap();

        let email2 = "unverified@test.com";
        let hash2 = test_blake3_hash(email2);
        db.insert_subscription(&hash2, email2).await.unwrap();

        let email3 = "verified2@test.com";
        let hash3 = test_blake3_hash(email3);
        db.insert_subscription(&hash3, email3).await.unwrap();
        db.verify_subscription(&hash3).await.unwrap();

        let verified = db.get_all_verified_subscribers().await.unwrap();

        assert_eq!(verified.len(), 2);
        assert!(verified.iter().any(|s| s.email == email1));
        assert!(verified.iter().any(|s| s.email == email3));
        assert!(!verified.iter().any(|s| s.email == email2));
    }

    #[tokio::test]
    async fn test_admin_broadcast_rejects_missing_key() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/admin/broadcast")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(
                        r#"{"template_path": "newsletters/1/body.html"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_admin_broadcast_enqueues_only_verified_subscribers() {
        let context = create_test_context().await;
        let db = context.db.clone();

        let verified_email = "broadcast-verified@test.com";
        let verified_hash = test_blake3_hash(verified_email);
        db.insert_subscription(&verified_hash, verified_email)
            .await
            .unwrap();
        db.verify_subscription(&verified_hash).await.unwrap();

        let unverified_email = "broadcast-unverified@test.com";
        let unverified_hash = test_blake3_hash(unverified_email);
        db.insert_subscription(&unverified_hash, unverified_email)
            .await
            .unwrap();

        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/admin/broadcast")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .header("x-admin-key", "test-admin-key")
                    .body(Body::from(
                        r#"{"template_path": "newsletters/1/body.html", "subject": "Edition #1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);

        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["queued"], 1);

        let pending = db.get_pending_outbox_items().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].email_type, "broadcast");
        assert_eq!(pending[0].recipient_email, verified_email);
        assert!(pending[0].broadcast_id.is_some());
        assert!(
            pending[0]
                .context_json
                .contains("http://localhost:3000/api/unsubscribe?token=")
        );
    }

    #[tokio::test]
    async fn test_admin_broadcast_unknown_template_is_rejected() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/admin/broadcast")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .header("x-admin-key", "test-admin-key")
                    .body(Body::from(
                        r#"{"template_path": "newsletters/999/body.html"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_verify_enqueues_welcome_email_once() {
        let context = create_test_context().await;
        let db = context.db.clone();
        let app = create_router(context);

        let email = "welcome@test.com";
        let subscribe_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(format!(r#"{{"email": "{email}"}}"#)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(subscribe_response.status(), StatusCode::OK);

        let token = hex::encode(test_blake3_hash(email));

        let first_verify = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri(format!("/api/verify?token={token}"))
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_verify.status(), StatusCode::OK);

        let pending = db.get_pending_outbox_items().await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(
            pending
                .iter()
                .filter(|item| item.email_type == "welcome")
                .count(),
            1
        );

        // Clicking the verification link again must not queue a second welcome.
        let second_verify = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri(format!("/api/verify?token={token}"))
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_verify.status(), StatusCode::OK);

        let pending_after = db.get_pending_outbox_items().await.unwrap();
        assert_eq!(
            pending_after
                .iter()
                .filter(|item| item.email_type == "welcome")
                .count(),
            1
        );
    }

    fn test_blake3_hash(email: &str) -> [u8; 32] {
        let key = test_blake3_key();
        let mut hasher = blake3::Hasher::new_keyed(&key);
        hasher.update(email.as_bytes());
        *hasher.finalize().as_bytes()
    }

    #[tokio::test]
    async fn test_rate_limit() {
        let context = create_test_context().await;
        let app = create_router(context);
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 5000);
        let listener = tokio::net::TcpListener::bind(socket).await.unwrap();

        let server = axum::serve(listener, app);
        let _handle = tokio::spawn(async move {
            server.await.expect("Test server failed to run");
        });
        wait_for_server_ready(&socket).await;

        let http_client = reqwest::Client::new();
        for i in 0..7 {
            let result = http_client
                .post("http://127.0.0.1:5000/api/subscribe")
                .header("X-Forwarded-For", "127.0.0.1")
                .header(
                    axum::http::header::CONTENT_TYPE,
                    mime::APPLICATION_JSON.as_ref(),
                )
                .body(r#"{"email": "test@email.de"}"#)
                .send()
                .await;

            assert!(result.is_ok());

            if i < 5 {
                assert_eq!(result.unwrap().status(), StatusCode::OK);
            } else {
                assert_ne!(result.unwrap().status(), StatusCode::OK);
            }
        }
    }

    #[tokio::test]
    async fn test_empty_email_error() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(r#"{"email": ""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_invalid_email_error() {
        let app = create_router(create_test_context().await);

        let verylong_email = format!("verylong{}", "x".repeat(300));
        let invalid_emails: Vec<&str> = vec!["noat", "ab@", "a@b", &verylong_email];

        for email in invalid_emails {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(axum::http::Method::POST)
                        .uri("/api/subscribe")
                        .header(
                            axum::http::header::CONTENT_TYPE,
                            mime::APPLICATION_JSON.as_ref(),
                        )
                        .header("X-Forwarded-For", "127.0.0.1")
                        .body(Body::from(format!(r#"{{"email": "{}"}}"#, email)))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(
                json["error"]
                    .as_str()
                    .unwrap()
                    .contains("Invalid email format")
            );
        }
    }

    #[tokio::test]
    async fn test_empty_token_verify_error() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/api/verify?token=")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_subscription_not_found_verify_error() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/api/verify?token=nonexistenttoken")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_empty_token_unsubscribe_error() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/api/unsubscribe?token=")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_subscription_not_found_unsubscribe_error() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri("/api/unsubscribe?token=nonexistenttoken")
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_malformed_json_error() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(String::from("{invalid json}")))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_error_response_format() {
        let app = create_router(create_test_context().await);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .header("X-Forwarded-For", "127.0.0.1")
                    .body(Body::from(r#"{"email": ""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json.is_object());
        assert!(json.get("error").is_some());
        assert!(json.get("code").is_some());
        assert_eq!(json["code"], 400);
    }

    #[tokio::test]
    async fn test_error_status_code_mapping() {
        let app = create_router(create_test_context().await);

        let test_cases = vec![
            (r#"{"email": ""}"#, StatusCode::BAD_REQUEST, "empty"),
            (
                r#"{"email": "invalid"}"#,
                StatusCode::BAD_REQUEST,
                "invalid format",
            ),
        ];

        for (body, expected_status, description) in test_cases {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(axum::http::Method::POST)
                        .uri("/api/subscribe")
                        .header(
                            axum::http::header::CONTENT_TYPE,
                            mime::APPLICATION_JSON.as_ref(),
                        )
                        .header("X-Forwarded-For", "127.0.0.1")
                        .body(Body::from(String::from(body)))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                expected_status,
                "Failed for {}",
                description
            );
        }
    }

    async fn wait_for_server_ready(addr: &SocketAddr) {
        let mut attempts = 0;
        const MAX_ATTEMPTS: u8 = 10;
        const DELAY: Duration = Duration::from_millis(50);

        loop {
            match TcpStream::connect(addr) {
                Ok(_) => {
                    // Connection successful, server is likely up
                    return;
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        panic!("Server failed to start after {} attempts.", MAX_ATTEMPTS);
                    }
                    tokio::time::sleep(DELAY).await;
                }
            }
        }
    }
}
