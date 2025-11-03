mod tests {
    const HEX_KEY: &str = "327da54c32bbda6f1b56c2e248620d31324b6bf5664fc31ab4d455f38787a5fa";

    use axum::{body::Body, extract::Request, http::StatusCode};
    use bytes::Bytes;
    use email_serv::http::ApiContext;
    use email_serv::http::SubscriptionEmail;
    use email_serv::http::create_router;
    use http_body_util::Full;
    use mime::Mime;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::u8;
    use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready

    #[tokio::test]
    async fn test_fallback() {
        let mut array = [0u8; 32];
        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let app = create_router(ApiContext {
            emails: Arc::new(Mutex::new(HashMap::new())),
            blake3_key: array,
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/gibberish")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_health_check() {
        let mut array = [0u8; 32];
        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let app = create_router(ApiContext {
            emails: Arc::new(Mutex::new(HashMap::new())),
            blake3_key: array,
        });
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health_check")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_unsubscribe() {
        let mut array = [0u8; 32];
        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let email = String::from("test@email.com");
        let mut hasher = blake3::Hasher::new_keyed(&array);
        hasher.update(email.clone().as_bytes());
        let hash = hasher.finalize().to_hex().to_string();
        let mut hashmap = HashMap::new();
        hashmap.insert(
            hash.clone(),
            SubscriptionEmail {
                email: email,
                is_verified: false,
            },
        );
        let context = ApiContext {
            emails: Arc::new(Mutex::new(hashmap)),
            blake3_key: array,
        };
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri(format!("/api/unsubscribe?token={}", hash))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_verify() {
        let mut array = [0u8; 32];
        let email = String::from("test@email.com");

        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let mut hasher = blake3::Hasher::new_keyed(&array);
        hasher.update(email.clone().as_bytes());
        let hash = hasher.finalize().to_hex().to_string();
        let mut hashmap = HashMap::new();
        hashmap.insert(
            hash.clone(),
            SubscriptionEmail {
                email: email,
                is_verified: false,
            },
        );
        let context = ApiContext {
            emails: Arc::new(Mutex::new(hashmap)),
            blake3_key: array,
        };

        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::GET)
                    .uri(format!("/api/verify?token={}", hash))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_subscribe() {
        let mut array = [0u8; 32];
        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let context = ApiContext {
            emails: Arc::new(Mutex::new(HashMap::new())),
            blake3_key: array,
        };
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
                    .body(Body::from(String::from("{\"email\": \"test@email.de\"}")))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_length() {
        let mut array = [0u8; 32];
        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let context = ApiContext {
            emails: Arc::new(Mutex::new(HashMap::new())),
            blake3_key: array,
        };
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .header(
                        axum::http::header::CONTENT_TYPE,
                        mime::APPLICATION_JSON.as_ref(),
                    )
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
}
