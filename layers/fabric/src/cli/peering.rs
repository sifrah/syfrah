use crate::control::{send_control_request, ControlRequest, ControlResponse};
use crate::sanitize::sanitize;
use crate::store;
use crate::ui;
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
        let sp = ui::spinner("No mesh configured. Creating one automatically...");
        crate::daemon::auto_init(&node_name, 51820, 51821)?;
        ui::step_ok(&sp, "Mesh auto-created");
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
            region: None,
            zone: None,
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

    ui::peering_banner(51821, pin.as_deref());

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

                let key_prefix = &req.wg_public_key[..20.min(req.wg_public_key.len())];
                ui::join_request_card(&sanitize(&req.node_name), &req.endpoint.to_string(), key_prefix);

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
                                if ui::is_tty() {
                                    let green = console::Style::new().green();
                                    println!(
                                        "     {} {} joined the mesh.\n",
                                        green.apply_to("\u{2713}"),
                                        sanitize(&peer_name)
                                    );
                                } else {
                                    println!("  Accepted: {} joined the mesh.\n", sanitize(&peer_name));
                                }
                            }
                            Ok(ControlResponse::Error { message }) => {
                                ui::warn(&format!("Error: {message}"));
                                println!();
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
                            Err(e) => {
                                ui::warn(&format!("Error: {e}"));
                                println!();
                            }
                        }
                    }
                }
            }
        }
    }
}

pub async fn start(port: u16, pin: Option<String>) -> Result<()> {
    let sp = ui::spinner(&format!("Starting peering on port {port}..."));
    let resp = send_request(ControlRequest::PeeringStart {
        port,
        pin: pin.clone(),
    })
    .await?;
    match resp {
        ControlResponse::Ok => {
            if let Some(ref p) = pin {
                ui::step_ok(&sp, &format!("Peering started on port {port}"));
                println!("  Mode: auto-accept with PIN");
                println!("  Nodes can join with: syfrah fabric join <this-ip> --pin {p}");
            } else {
                ui::step_ok(&sp, &format!("Peering started on port {port}"));
                println!("  Mode: manual approval (you must accept each join request)");
            }
        }
        ControlResponse::Error { message } => {
            ui::step_fail(&sp, &format!("Failed: {message}"));
            anyhow::bail!("{message}");
        }
        _ => {
            ui::step_fail(&sp, "Unexpected response");
            anyhow::bail!("unexpected response");
        }
    }
    Ok(())
}

pub async fn stop() -> Result<()> {
    let sp = ui::spinner("Stopping peering...");
    let resp = send_request(ControlRequest::PeeringStop).await?;
    match resp {
        ControlResponse::Ok => ui::step_ok(&sp, "Peering stopped."),
        ControlResponse::Error { message } => {
            ui::step_fail(&sp, &format!("Failed: {message}"));
            anyhow::bail!("{message}");
        }
        _ => {
            ui::step_fail(&sp, "Unexpected response");
            anyhow::bail!("unexpected response");
        }
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
                        truncate(&sanitize(&r.node_name), 15),
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
    let sp = ui::spinner(&format!("Accepting request {request_id}..."));
    let resp = send_request(ControlRequest::PeeringAccept {
        request_id: request_id.to_string(),
    })
    .await?;
    match resp {
        ControlResponse::PeeringAccepted { peer_name } => {
            ui::step_ok(&sp, &format!("{} joined the mesh.", sanitize(&peer_name)));
        }
        ControlResponse::Error { message } => {
            ui::step_fail(&sp, &format!("Failed: {message}"));
            anyhow::bail!("{message}");
        }
        _ => {
            ui::step_fail(&sp, "Unexpected response");
            anyhow::bail!("unexpected response");
        }
    }
    Ok(())
}

pub async fn reject(request_id: &str, reason: Option<String>) -> Result<()> {
    let sp = ui::spinner(&format!("Rejecting request {request_id}..."));
    let resp = send_request(ControlRequest::PeeringReject {
        request_id: request_id.to_string(),
        reason,
    })
    .await?;
    match resp {
        ControlResponse::Ok => ui::step_ok(&sp, &format!("Request {request_id} rejected.")),
        ControlResponse::Error { message } => {
            ui::step_fail(&sp, &format!("Failed: {message}"));
            anyhow::bail!("{message}");
        }
        _ => {
            ui::step_fail(&sp, "Unexpected response");
            anyhow::bail!("unexpected response");
        }
    }
    Ok(())
}

async fn send_request(req: ControlRequest) -> Result<ControlResponse> {
    let path = store::control_socket_path();
    if !path.exists() {
        anyhow::bail!("daemon not running. Start with 'syfrah fabric start' first.");
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
