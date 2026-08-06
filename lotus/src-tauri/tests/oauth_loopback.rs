//! Drives the loopback OAuth listener over a real TCP socket.
//!
//! Everything up to the token exchange is exercised here: bind, browser
//! redirect, `state` validation, code extraction, and the browser response. The
//! token exchange itself talks to Google and is covered by the unit tests on
//! `classify` and the manual smoke run.
//!
//! This is the mechanism most likely to fail in a way unit tests miss, because
//! it involves a socket, an HTTP request the code parses by hand, and a
//! constant-time comparison.

use std::time::Duration;

use lotus_app_lib::testing::{begin_login, ClientConfig};

/// Send a raw HTTP GET to the loopback listener and return its response body.
async fn redirect_to(port: u16, target: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("the callback listener must accept a connection");
    stream
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
        .await
        .unwrap();
    stream.flush().await.unwrap();

    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response)).await;
    String::from_utf8_lossy(&response).into_owned()
}

fn config() -> ClientConfig {
    ClientConfig {
        client_id: "test-client-id.apps.googleusercontent.com".into(),
        client_secret: "test-secret".into(),
    }
}

fn port_of(redirect_uri: &str) -> u16 {
    redirect_uri
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .expect("the redirect URI must end in a port")
}

#[tokio::test]
async fn a_matching_callback_yields_the_authorization_code() {
    let pending = begin_login(&config()).await.unwrap();
    let port = port_of(&pending.redirect_uri);
    let state = pending.state.clone();

    let waiting = tokio::spawn(async move { pending.wait_for_code().await });

    let body = redirect_to(port, &format!("/?code=4%2F0AbCd_efg&state={state}")).await;
    assert!(body.contains("200 OK"));
    assert!(
        body.contains("close this tab"),
        "the browser should get a friendly page: {body}"
    );

    let (code, verifier, redirect_uri) = waiting.await.unwrap().unwrap();
    // Percent-decoded: Google codes contain a literal slash.
    assert_eq!(code, "4/0AbCd_efg");
    assert_eq!(verifier.len(), 86);
    assert!(redirect_uri.starts_with("http://127.0.0.1:"));
}

/// A replayed or forged callback must be refused, and no code returned.
#[tokio::test]
async fn a_mismatched_state_is_rejected() {
    let pending = begin_login(&config()).await.unwrap();
    let port = port_of(&pending.redirect_uri);

    let waiting = tokio::spawn(async move { pending.wait_for_code().await });

    let body = redirect_to(port, "/?code=stolen&state=not-the-right-state").await;
    assert!(body.contains("not valid"));

    let error = waiting.await.unwrap().expect_err("must not return a code");
    assert!(
        error.contains("did not match"),
        "the message should say the response did not match: {error}"
    );
    assert!(error.contains("No account was connected"));
}

/// A user who clicks "Cancel" on the consent screen gets `error=access_denied`.
/// That is not a failure to report as a bug.
#[tokio::test]
async fn a_cancelled_consent_reads_as_cancelled() {
    let pending = begin_login(&config()).await.unwrap();
    let port = port_of(&pending.redirect_uri);

    let waiting = tokio::spawn(async move { pending.wait_for_code().await });
    redirect_to(port, "/?error=access_denied&state=whatever").await;

    let error = waiting.await.unwrap().expect_err("must not return a code");
    assert!(error.contains("cancelled"), "got: {error}");
}

/// The browser hits /favicon.ico against the same port. Treating that as the
/// redirect would abort the flow before the user finished consenting.
#[tokio::test]
async fn an_unrelated_request_does_not_end_the_flow() {
    let pending = begin_login(&config()).await.unwrap();
    let port = port_of(&pending.redirect_uri);
    let state = pending.state.clone();

    let waiting = tokio::spawn(async move { pending.wait_for_code().await });

    let favicon = redirect_to(port, "/favicon.ico").await;
    assert!(favicon.contains("Waiting for Google"));

    // The listener is still up, so the real redirect still works.
    redirect_to(port, &format!("/?code=real-code&state={state}")).await;
    let (code, _, _) = waiting.await.unwrap().unwrap();
    assert_eq!(code, "real-code");
}

/// A callback with `state` but no `code` is malformed and must not be treated as
/// a success with an empty code.
#[tokio::test]
async fn a_callback_without_a_code_is_refused() {
    let pending = begin_login(&config()).await.unwrap();
    let port = port_of(&pending.redirect_uri);
    let state = pending.state.clone();

    let waiting = tokio::spawn(async move { pending.wait_for_code().await });
    redirect_to(port, &format!("/?state={state}")).await;

    let error = waiting.await.unwrap().expect_err("must not return a code");
    assert!(error.contains("authorization code"), "got: {error}");
}

/// Two concurrent logins get different ports and different states, so one
/// cannot satisfy the other's listener.
#[tokio::test]
async fn concurrent_logins_do_not_share_a_port_or_a_state() {
    let first = begin_login(&config()).await.unwrap();
    let second = begin_login(&config()).await.unwrap();

    assert_ne!(first.redirect_uri, second.redirect_uri);
    assert_ne!(first.state, second.state);
    assert_ne!(first.verifier, second.verifier);
}
