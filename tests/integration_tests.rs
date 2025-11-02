mod tests {
    const HEX_KEY: &str = "327da54c32bbda6f1b56c2e248620d31324b6bf5664fc31ab4d455f38787a5fa";

    use axum::{body::Body, extract::Request, http::StatusCode};
    use email_serv::http::ApiContext;
    use email_serv::http::create_router;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::u8;
    use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`

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
        let context = ApiContext {
            emails: Arc::new(Mutex::new(HashMap::new())),
            blake3_key: array,
        };
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/api/unsubscribe")
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
                    .method(axum::http::Method::GET)
                    .uri("/api/subscribe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
