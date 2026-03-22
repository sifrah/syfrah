use crate::control::{send_control_request, ControlRequest, ControlResponse};
use crate::store;
use anyhow::Result;
use std::collections::HashSet;

/// Interactive peering mode: watch for requests and prompt accept/reject.
pub async fn watch(pin: Option<String>) -> Result<()> {
    // Auto-init if no mesh exists
    if !store::exists() {
        let node_name = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "syfrah-node".into());
        println!("No mesh configured. Creating one automatically...");
        crate::daemon::auto_init(&node_name, 51820, 51821)?;
        println!();

        // Start daemon in background
        let mesh_secret: syfrah_core::secret::MeshSecret = store::load()?
            .mesh_secret
            .parse()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let wg_private = wireguard_control::Key::from_base64(&store::load()?.wg_private_key)
            .map_err(|_| anyhow::anyhow!("corrupt WG key"))?;
        let wg_keypair = wireguard_control::KeyPair::from_private(wg_private);
        let state = store::load()?;
        let endpoint = state.public_endpoint.unwrap_or_else(|| {
            std::net::SocketAddr::new("0.0.0.0".parse().unwrap(), state.wg_listen_port)
        });
        let my_record = syfrah_core::mesh::PeerRecord {
            name: state.node_name.clone(),
            wg_public_key: wg_keypair.public.to_base64(),
            endpoint,
            mesh_ipv6: state.mesh_ipv6,
            last_seen: 0,
            status: syfrah_core::mesh::PeerStatus::Active,
        };
        let pp = state.peering_port;
        tokio::spawn(async move {
            if let Err(e) = crate::daemon::run_daemon(my_record, &wg_keypair, mesh_secret, pp).await
            {
                eprintln!("daemon error: {e}");
            }
        });
        // Wait for control socket
        for _ in 0..30 {
            if store::control_socket_path().exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // Start peering with optional PIN
    let resp = send_request(ControlRequest::PeeringStart {
        port: 51821,
        pin: pin.clone(),
    })
    .await?;
    match resp {
        ControlResponse::Ok => {}
        ControlResponse::Error { message } => anyhow::bail!("{message}"),
        _ => {}
    }

    if let Some(ref p) = pin {
        println!("Peering active (auto-accept with PIN: {p})");
        println!("New nodes can join with: syfrah join <this-ip> --pin {p}");
    } else {
        println!("Peering active. Watching for join requests...");
        println!("Press Ctrl+C to stop.");
    }
    println!();

    // Poll for new requests
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let resp = match send_request(ControlRequest::PeeringList).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        if let ControlResponse::PeeringList { requests } = resp {
            for req in &requests {
                if seen.contains(&req.request_id) {
                    continue;
                }
                seen.insert(req.request_id.clone());

                println!("Join request from {} ({})", req.node_name, req.endpoint);
                println!(
                    "  WG pubkey: {}",
                    &req.wg_public_key[..20.min(req.wg_public_key.len())]
                );
                print!("  Accept? [Y/n] ");

                // Read from stdin
                use std::io::Write;
                std::io::stdout().flush().ok();
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_ok() {
                    let trimmed = input.trim().to_lowercase();
                    if trimmed.is_empty() || trimmed == "y" || trimmed == "yes" {
                        match send_request(ControlRequest::PeeringAccept {
                            request_id: req.request_id.clone(),
                        })
                        .await
                        {
                            Ok(ControlResponse::PeeringAccepted { peer_name }) => {
                                println!("  Accepted: {peer_name} joined the mesh.\n");
                            }
                            Ok(ControlResponse::Error { message }) => {
                                println!("  Error: {message}\n");
                            }
                            _ => {}
                        }
                    } else {
                        match send_request(ControlRequest::PeeringReject {
                            request_id: req.request_id.clone(),
                            reason: Some("rejected by operator".into()),
                        })
                        .await
                        {
                            Ok(_) => println!("  Rejected.\n"),
                            Err(e) => println!("  Error: {e}\n"),
                        }
                    }
                }
            }
        }
    }
}

pub async fn start(port: u16, pin: Option<String>) -> Result<()> {
    let resp = send_request(ControlRequest::PeeringStart {
        port,
        pin: pin.clone(),
    })
    .await?;
    match resp {
        ControlResponse::Ok => {
            if let Some(p) = pin {
                println!("Peering started on port {port} (auto-accept PIN: {p}).");
            } else {
                println!("Peering started on port {port}.");
            }
        }
        ControlResponse::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("unexpected response"),
    }
    Ok(())
}

pub async fn stop() -> Result<()> {
    let resp = send_request(ControlRequest::PeeringStop).await?;
    match resp {
        ControlResponse::Ok => println!("Peering stopped."),
        ControlResponse::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("unexpected response"),
    }
    Ok(())
}

pub async fn list() -> Result<()> {
    let resp = send_request(ControlRequest::PeeringList).await?;
    match resp {
        ControlResponse::PeeringList { requests } => {
            if requests.is_empty() {
                println!("No pending join requests.");
            } else {
                println!(
                    "{:<10} {:<16} {:<22} {:<20}",
                    "ID", "NAME", "ENDPOINT", "WG PUBKEY"
                );
                println!("{}", "-".repeat(70));
                for r in &requests {
                    println!(
                        "{:<10} {:<16} {:<22} {:<20}",
                        r.request_id,
                        truncate(&r.node_name, 15),
                        r.endpoint,
                        truncate(&r.wg_public_key, 19),
                    );
                }
                println!("\n{} pending request(s)", requests.len());
            }
        }
        ControlResponse::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("unexpected response"),
    }
    Ok(())
}

pub async fn accept(request_id: &str) -> Result<()> {
    let resp = send_request(ControlRequest::PeeringAccept {
        request_id: request_id.to_string(),
    })
    .await?;
    match resp {
        ControlResponse::PeeringAccepted { peer_name } => {
            println!("Accepted: {peer_name} joined the mesh.");
        }
        ControlResponse::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("unexpected response"),
    }
    Ok(())
}

pub async fn reject(request_id: &str, reason: Option<String>) -> Result<()> {
    let resp = send_request(ControlRequest::PeeringReject {
        request_id: request_id.to_string(),
        reason,
    })
    .await?;
    match resp {
        ControlResponse::Ok => println!("Request {request_id} rejected."),
        ControlResponse::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("unexpected response"),
    }
    Ok(())
}

async fn send_request(req: ControlRequest) -> Result<ControlResponse> {
    let path = store::control_socket_path();
    if !path.exists() {
        anyhow::bail!("daemon not running. Start with 'syfrah start' first.");
    }
    let resp = send_control_request(&path, &req)
        .await
        .map_err(|e| anyhow::anyhow!("failed to communicate with daemon: {e}"))?;
    Ok(resp)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}
