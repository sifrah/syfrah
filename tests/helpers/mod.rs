//! Shared utilities for E2E tests.
//!
//! Each `TestNode` simulates a syfrah node with its own temp directory,
//! unique ports, and WireGuard interface. Multiple TestNodes on the same
//! machine form a simulated cluster for testing.

// TODO: Implement TestNode when fabric layer supports programmatic init/join.
//
// pub struct TestNode {
//     pub name: String,
//     pub wg_port: u16,
//     pub peering_port: u16,
//     pub api_port: u16,
//     pub data_dir: tempfile::TempDir,
//     pub mesh_ipv6: Option<std::net::Ipv6Addr>,
// }
