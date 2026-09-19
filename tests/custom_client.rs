//! Tests for the caller-supplied client API.
//!
//! These tests never leave the loopback interface: `LookupProvider::IpApiCom`
//! has a plain-HTTP endpoint, so an HTTP proxy pointed at a local `TcpListener`
//! receives the full request and can answer it with a canned body.

use public_ip_address::lookup::{Client, LookupProvider, LookupService};
use reqwest::Proxy;
use serial_test::serial;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// A canned <http://ip-api.com/json/> reply for 203.0.113.7 (TEST-NET-3).
const IP_API_COM_BODY: &str = r#"{"query":"203.0.113.7","status":"success","continent":"Europe","continentCode":"EU","country":"Netherlands","countryCode":"NL","region":"NH","regionName":"North Holland","city":"Amsterdam","zip":"1011","lat":52.3759,"lon":4.8975,"timezone":"Europe/Amsterdam","org":"Example Org","as":"AS64496 Example","reverse":"example.invalid","mobile":false,"proxy":false,"hosting":false}"#;

/// Starts a loopback HTTP server that answers every connection with `body`.
///
/// Returns the address it listens on and a counter of accepted connections.
fn spawn_http_server(body: &'static str) -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
    let addr = listener.local_addr().expect("listener address");
    let hits = Arc::new(AtomicUsize::new(0));
    let thread_hits = Arc::clone(&hits);

    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => break,
            };
            thread_hits.fetch_add(1, Ordering::SeqCst);
            // Drain the request line and headers so the peer is not reset.
            let mut buffer = [0u8; 8192];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });

    (addr, hits)
}

#[maybe_async::test(feature = "blocking", async(not(feature = "blocking"), tokio::test))]
#[serial]
async fn test_lookup_service_with_client_uses_the_given_client() {
    let (addr, hits) = spawn_http_server(IP_API_COM_BODY);
    let client = Client::builder()
        .proxy(Proxy::http(format!("http://{}", addr)).expect("proxy url"))
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build client");

    let service = LookupService::with_client(LookupProvider::IpApiCom, None, &client);
    let response = service
        .lookup(None)
        .await
        .expect("lookup succeeds against the loopback server");

    assert_eq!(response.ip, "203.0.113.7".parse::<IpAddr>().unwrap());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}
