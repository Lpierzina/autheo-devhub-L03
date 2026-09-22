//! Private Marketplace container gateway.
//!
//! Marketplace applications speak ordinary HTTPS to a node-local, private
//! listener.  The listener exposes only the Marketplace service contract and
//! translates the request into a signed-Iroh gossip request.  Applications
//! never receive mesh identities, tickets, relay URLs, or node addresses.
//!
//! The gateway deliberately does *not* verify Marketplace HMAC itself.  The
//! complete original method, path, allowlisted headers, and raw body are
//! delivered to [`crate::marketplace::mesh_dispatch`], where the existing
//! router-level HMAC middleware verifies them exactly once.

use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderName, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

use crate::state::CloudState;

const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MESH_PATH: &str = "/v1/marketplace/gateway-mesh";

/// The sole wire shape carried through the Hive mesh for this gateway.  Do not
/// add arbitrary headers: headers outside this list are neither needed by the
/// Marketplace contract nor safe to replay across a trust boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MarketplaceMeshRequest {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body_b64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MarketplaceMeshResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body_b64: String,
}

fn allowed_path(path: &str) -> bool {
    matches!(
        path,
        "/v1/marketplace/l0/deployments"
            | "/v1/marketplace/payment-intents"
            | "/v1/marketplace/payments/verify"
            | "/v1/marketplace/l0/allocations"
    )
}

fn allowed_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "x-marketplace-key-id"
            | "x-marketplace-timestamp"
            | "x-marketplace-nonce"
            | "x-marketplace-content-sha256"
            | "x-marketplace-signature"
            | "idempotency-key"
            | "content-type"
    )
}

fn mesh_request(parts: &axum::http::request::Parts, body: &[u8]) -> Option<MarketplaceMeshRequest> {
    let path = parts.uri.path();
    if !allowed_path(path) {
        return None;
    }
    let method = parts.method.as_str();
    if !matches!(method, "GET" | "POST") {
        return None;
    }
    let mut headers = BTreeMap::new();
    for (name, value) in &parts.headers {
        if allowed_header(name) {
            headers.insert(name.as_str().to_owned(), value.to_str().ok()?.to_owned());
        }
    }
    Some(MarketplaceMeshRequest {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body_b64: STANDARD.encode(body),
    })
}

fn response_from_mesh(reply: MarketplaceMeshResponse) -> Response {
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let body = STANDARD.decode(reply.body_b64).unwrap_or_default();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Some(content_type) = reply
        .content_type
        .and_then(|value| value.parse::<HeaderValue>().ok())
    {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, content_type);
    }
    response
}

fn gateway_error(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        format!(r#"{{"error":"{code}"}}"#),
    )
        .into_response()
}

async fn gateway_request(State(cloud): State<Arc<CloudState>>, req: Request<Body>) -> Response {
    let (parts, body) = req.into_parts();
    let body = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return gateway_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "marketplace_request_too_large",
            )
        }
    };
    let Some(request) = mesh_request(&parts, &body) else {
        return gateway_error(StatusCode::NOT_FOUND, "marketplace_gateway_route_not_found");
    };
    let request_bytes = match serde_json::to_vec(&request) {
        Ok(value) => value,
        Err(_) => {
            return gateway_error(StatusCode::BAD_GATEWAY, "marketplace_gateway_encode_failed")
        }
    };

    // A local delivery remains mesh-equivalent: it reconstructs a fresh request
    // and enters the same Marketplace router, including its raw-body HMAC gate.
    // This is the Genesis path and avoids manufacturing a self-dial.
    let targets = crate::marketplace::gateway_targets(&cloud);
    if targets.is_empty() {
        return gateway_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "marketplace_gateway_unavailable",
        );
    }

    let is_read = request.method == "GET";
    for (index, target) in targets.iter().enumerate() {
        let reply = if target == &cloud.node_name {
            crate::marketplace::mesh_dispatch(cloud.clone(), request.clone()).await
        } else {
            let peer = cloud
                .registry
                .nodes()
                .into_iter()
                .find(|node| node.id == target.as_str() || node.name == target.as_str());
            let Some(peer) = peer.and_then(|node| Some((node.peer_id?, node.iroh_addr?))) else {
                None
            };
            match peer {
                Some((peer_id, addr)) => crate::gossip::request_to_with_response_cap(
                    &cloud,
                    &peer_id,
                    &addr,
                    hive_p2p::GOSSIP_POST,
                    MESH_PATH,
                    &request_bytes,
                    20,
                    MAX_RESPONSE_BYTES,
                )
                .await
                .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
                None => None,
            }
        };
        if let Some(reply) = reply {
            return response_from_mesh(reply);
        }
        // A write may have reached the peer before its response was lost.  Its
        // idempotency key is the only safe retry mechanism, so never silently
        // replay it to another node.  GET is side-effect free and can fail over.
        if !is_read || index + 1 == targets.len() {
            break;
        }
    }
    gateway_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "marketplace_gateway_unavailable",
    )
}

/// Private listener router.  Deliberately separate from Admin: no Admin,
/// dashboard, tenant-edge, or catch-all routes are reachable through it.
fn routes(cloud: Arc<CloudState>) -> Router {
    Router::new()
        .route("/v1/marketplace/l0/deployments", get(gateway_request))
        .route("/v1/marketplace/payment-intents", post(gateway_request))
        .route("/v1/marketplace/payments/verify", post(gateway_request))
        .route("/v1/marketplace/l0/allocations", post(gateway_request))
        .with_state(cloud)
}

/// Starts only when all gateway TLS/mTLS settings are configured. The address
/// must be the Marketplace project's RFC1918 Podman bridge address; loopback,
/// wildcard, link-local, and public binds are refused.
pub fn spawn(cloud: Arc<CloudState>) {
    let listen = match std::env::var("HIVE_MARKETPLACE_GATEWAY_LISTEN") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return,
    };
    let cert = match std::env::var("HIVE_MARKETPLACE_GATEWAY_TLS_CERT") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::error!(
                "Marketplace gateway disabled: HIVE_MARKETPLACE_GATEWAY_TLS_CERT is required"
            );
            return;
        }
    };
    let key = match std::env::var("HIVE_MARKETPLACE_GATEWAY_TLS_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::error!(
                "Marketplace gateway disabled: HIVE_MARKETPLACE_GATEWAY_TLS_KEY is required"
            );
            return;
        }
    };
    let client_ca = match std::env::var("HIVE_MARKETPLACE_GATEWAY_CA_CERT") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            tracing::error!(
                "Marketplace gateway disabled: HIVE_MARKETPLACE_GATEWAY_CA_CERT is required"
            );
            return;
        }
    };
    let Ok(addr) = listen.parse::<SocketAddr>() else {
        tracing::error!("Marketplace gateway disabled: HIVE_MARKETPLACE_GATEWAY_LISTEN is invalid");
        return;
    };
    let private_bridge = match addr.ip() {
        std::net::IpAddr::V4(ip) => ip.is_private() && !ip.is_loopback() && !ip.is_link_local(),
        std::net::IpAddr::V6(_) => false,
    };
    if !private_bridge {
        tracing::error!(%addr, "Marketplace gateway disabled: listener must use an RFC1918 Podman bridge address");
        return;
    }
    tokio::spawn(async move {
        let config =
            match mtls_config(Path::new(&cert), Path::new(&key), Path::new(&client_ca)).await {
                Ok(config) => config,
                Err(error) => {
                    tracing::error!(%error, "Marketplace gateway mTLS configuration failed");
                    return;
                }
            };
        tracing::info!(%addr, "private Marketplace HTTPS gateway listening");
        if let Err(error) = axum_server::bind_rustls(addr, config)
            .serve(routes(cloud).into_make_service_with_connect_info::<SocketAddr>())
            .await
        {
            tracing::error!(%error, "private Marketplace HTTPS gateway stopped");
        }
    });
}

async fn mtls_config(
    cert_path: &Path,
    key_path: &Path,
    ca_path: &Path,
) -> anyhow::Result<axum_server::tls_rustls::RustlsConfig> {
    let cert_pem = tokio::fs::read(cert_path).await?;
    let key_pem = tokio::fs::read(key_path).await?;
    let ca_pem = tokio::fs::read(ca_path).await?;
    let certs = rustls_pemfile::certs(&mut cert_pem.as_slice()).collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(!certs.is_empty(), "gateway certificate chain is empty");
    let key = rustls_pemfile::private_key(&mut key_pem.as_slice())?
        .ok_or_else(|| anyhow::anyhow!("gateway private key is missing"))?;
    let mut roots = rustls::RootCertStore::empty();
    let ca_certs = rustls_pemfile::certs(&mut ca_pem.as_slice()).collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(!ca_certs.is_empty(), "Marketplace client CA is empty");
    for ca in ca_certs {
        roots.add(ca)?;
    }
    let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots)).build()?;
    let mut config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(axum_server::tls_rustls::RustlsConfig::from_config(
        Arc::new(config),
    ))
}
