mod tests {
    use axum::{body::Body, extract::Request, http::StatusCode};
    use tower::{Service, ServiceExt}; // for `call`, `oneshot`, and `ready`
    use email_serv::http::create_router;
    use email_serv::http::ApiContext;
    use std::sync::Arc;
    use parking_lot::Mutex;

    #[tokio::test]
    async fn test_fallback() {
        let app = create_router(ApiContext {
            emails: Arc::new(Mutex::new(Vec::new())),
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
        let app = create_router(ApiContext {
            emails: Arc::new(Mutex::new(Vec::new())),
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
        let context = ApiContext {
            emails: Arc::new(Mutex::new(Vec::new())),
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
        let context = ApiContext {
            emails: Arc::new(Mutex::new(Vec::new())),
        };
        let app = create_router(context);
        let response = app
            .oneshot(
                Request::builder()
                     .method(axum::http::Method::POST)
                    .uri("/api/subscribe")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
