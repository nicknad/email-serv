mod tests {
    const HEX_KEY: &str = "327da54c32bbda6f1b56c2e248620d31324b6bf5664fc31ab4d455f38787a5fa";

    use axum::{body::Body, extract::Request, http::StatusCode};
    use bytes::Bytes;
    use email_serv::http::ApiContext;
    use email_serv::http::SubscriptionEmail;
    use email_serv::http::create_router;
    use http_body_util::Full;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::net::TcpListener;
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
                    .header("X-Forwarded-For", "127.0.0.1")
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
                    .header("X-Forwarded-For", "127.0.0.1")
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
                    .header("X-Forwarded-For", "127.0.0.1")
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

    #[tokio::test]
    async fn test_rate_limit() {
        let mut array = [0u8; 32];
        hex::decode_to_slice(HEX_KEY, &mut array as &mut [u8]).unwrap();
        let context = ApiContext {
            emails: Arc::new(Mutex::new(HashMap::new())),
            blake3_key: array,
        };
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
                .body("{\"email\": \"test@email.de\"}")
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
