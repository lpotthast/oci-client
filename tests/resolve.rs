// `ClientConfig::resolve` lets the client reach a registry under its name at another address,
// for example a registry inside a container network that is published under a public name.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{http::HeaderMap, routing::get, Router};
use oci_client::{
    client::{ClientConfig, ClientProtocol},
    secrets::RegistryAuth,
    Client, Reference,
};
use tokio::net::TcpListener;

/// A name that never resolves (RFC 6761), so that only the override can reach the registry.
const REGISTRY_NAME: &str = "registry.invalid";

/// The `Host` header received by the tags endpoint.
type Seen = Arc<Mutex<Option<String>>>;

/// Serves a registry with one repository on the loopback address and returns the port.
async fn spawn_registry(seen: Seen) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = Router::new().route(
        "/v2/repo/tags/list",
        get(move |headers: HeaderMap| async move {
            let host = headers.get("Host").unwrap().to_str().unwrap().to_string();
            *seen.lock().unwrap() = Some(host);
            r#"{"name":"repo","tags":["latest"]}"#
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

#[tokio::test]
async fn resolve_connects_to_the_given_address_and_keeps_the_name() {
    let seen = Seen::default();
    let port = spawn_registry(seen.clone()).await;
    let client = Client::try_from(ClientConfig {
        protocol: ClientProtocol::Http,
        // The port of the address is ignored, the client connects to the port of the reference.
        resolve: HashMap::from([(
            REGISTRY_NAME.to_string(),
            vec![SocketAddr::from(([127, 0, 0, 1], 0))],
        )]),
        ..Default::default()
    })
    .unwrap();
    let reference: Reference = format!("{REGISTRY_NAME}:{port}/repo:latest")
        .parse()
        .unwrap();

    let tags = client
        .list_tags(&reference, &RegistryAuth::Anonymous, None, None)
        .await
        .unwrap();

    assert_eq!(tags.tags, vec!["latest".to_string()]);
    assert_eq!(
        seen.lock().unwrap().as_deref(),
        Some(format!("{REGISTRY_NAME}:{port}").as_str()),
    );
}

#[tokio::test]
async fn without_resolve_the_name_is_resolved() {
    let port = spawn_registry(Seen::default()).await;
    let client = Client::try_from(ClientConfig {
        protocol: ClientProtocol::Http,
        ..Default::default()
    })
    .unwrap();
    let reference: Reference = format!("{REGISTRY_NAME}:{port}/repo:latest")
        .parse()
        .unwrap();

    let tags = client
        .list_tags(&reference, &RegistryAuth::Anonymous, None, None)
        .await;

    assert!(tags.is_err());
}
