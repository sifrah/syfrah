//! Fake Cloud Hypervisor binary for E2E testing.
//!
//! Simulates the cloud-hypervisor HTTP-over-Unix-socket API so that the syfrah
//! compute layer can be exercised end-to-end inside Docker without KVM.

use std::path::PathBuf;
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use tokio::net::UnixListener;
use tokio::sync::watch;

// ---------------------------------------------------------------------------
// VM state (good enough for testing)
// ---------------------------------------------------------------------------

use std::sync::Mutex;

struct VmState {
    created: bool,
    booted: bool,
    paused: bool,
}

impl VmState {
    fn new() -> Self {
        Self {
            created: false,
            booted: false,
            paused: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Request handler
// ---------------------------------------------------------------------------

/// Returns (status, optional JSON body, should_schedule_exit).
fn handle(
    method: &Method,
    path: &str,
    state: &Mutex<VmState>,
) -> (StatusCode, Option<&'static str>, bool) {
    match (method, path) {
        (&Method::GET, "/api/v1/vmm.ping") => {
            (StatusCode::OK, Some(r#"{"build_version":"v43.0"}"#), false)
        }

        (&Method::PUT, "/api/v1/vm.create") => {
            let mut s = state.lock().unwrap();
            s.created = true;
            (StatusCode::NO_CONTENT, None, false)
        }

        (&Method::PUT, "/api/v1/vm.boot") => {
            let mut s = state.lock().unwrap();
            s.booted = true;
            s.paused = false;
            (StatusCode::NO_CONTENT, None, false)
        }

        (&Method::GET, "/api/v1/vm.info") => {
            let s = state.lock().unwrap();
            let st = if s.booted {
                if s.paused {
                    "Paused"
                } else {
                    "Running"
                }
            } else if s.created {
                "Created"
            } else {
                "NotCreated"
            };
            // We leak a small string for the static response — acceptable in a test fake.
            let json: &'static str = Box::leak(
                format!(
                    r#"{{"state":"{}","config":{{"cpus":{{"boot_vcpus":2,"max_vcpus":2}},"memory":{{"size":536870912}}}},"memory_actual_size":536870912}}"#,
                    st
                )
                .into_boxed_str(),
            );
            (StatusCode::OK, Some(json), false)
        }

        (&Method::PUT, "/api/v1/vm.shutdown") => {
            let mut s = state.lock().unwrap();
            s.booted = false;
            s.paused = false;
            (StatusCode::NO_CONTENT, None, true)
        }

        // vm.power-button is the forced shutdown variant (ACPI power button)
        (&Method::PUT, "/api/v1/vm.power-button") => {
            let mut s = state.lock().unwrap();
            s.booted = false;
            s.paused = false;
            (StatusCode::NO_CONTENT, None, true)
        }

        (&Method::PUT, "/api/v1/vm.delete") => {
            let mut s = state.lock().unwrap();
            s.created = false;
            s.booted = false;
            s.paused = false;
            (StatusCode::NO_CONTENT, None, false)
        }

        (&Method::PUT, "/api/v1/vm.reboot") => {
            let mut s = state.lock().unwrap();
            s.booted = true;
            s.paused = false;
            (StatusCode::NO_CONTENT, None, false)
        }

        (&Method::PUT, "/api/v1/vm.resize") => (StatusCode::NO_CONTENT, None, false),
        (&Method::PUT, "/api/v1/vm.pause") => {
            state.lock().unwrap().paused = true;
            (StatusCode::NO_CONTENT, None, false)
        }
        (&Method::PUT, "/api/v1/vm.resume") => {
            state.lock().unwrap().paused = false;
            (StatusCode::NO_CONTENT, None, false)
        }
        (&Method::GET, "/api/v1/vm.counters") => (StatusCode::OK, Some(r#"{}"#), false),

        _ => (StatusCode::NOT_FOUND, None, false),
    }
}

fn build_response(status: StatusCode, body: Option<&str>) -> Response<Full<Bytes>> {
    let builder = Response::builder().status(status);
    match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(json.to_owned())))
            .unwrap(),
        None => builder
            .header("content-length", "0")
            .body(Full::new(Bytes::new()))
            .unwrap(),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    // --version
    if args.iter().any(|a| a == "--version") {
        println!("cloud-hypervisor v43.0");
        return;
    }

    // --api-socket <path>
    let socket_path = args
        .windows(2)
        .find(|w| w[0] == "--api-socket")
        .map(|w| PathBuf::from(&w[1]))
        .expect("missing --api-socket argument");

    // Print PID (diagnostic, goes to stdout.log)
    println!("{}", std::process::id());

    // Clean up stale socket
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("failed to bind unix socket");

    let state = Arc::new(Mutex::new(VmState::new()));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let shutdown_tx = Arc::new(shutdown_tx);
    let socket_path_clone = socket_path.clone();

    // SIGTERM handler
    let shutdown_sig = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler")
            .recv()
            .await;
        let _ = shutdown_sig.send(true);
    });

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            result = listener.accept() => {
                let (stream, _) = match result {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let state = Arc::clone(&state);
                let shutdown_tx = Arc::clone(&shutdown_tx);

                tokio::spawn(async move {
                    let state = state;
                    let shutdown_tx = shutdown_tx;
                    let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                        let state = Arc::clone(&state);
                        let shutdown_tx = Arc::clone(&shutdown_tx);
                        async move {
                            let (status, body, should_exit) =
                                handle(req.method(), req.uri().path(), &state);
                            if should_exit {
                                tokio::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    let _ = shutdown_tx.send(true);
                                });
                            }
                            Ok::<_, hyper::Error>(build_response(status, body))
                        }
                    });

                    let io = hyper_util::rt::TokioIo::new(stream);
                    let _ = http1::Builder::new().serve_connection(io, service).await;
                });
            }
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(&socket_path_clone);
}
