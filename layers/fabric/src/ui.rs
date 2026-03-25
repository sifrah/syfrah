//! Rich terminal feedback helpers: spinners, styled output, TTY detection.
//!
//! When stdout is not a terminal (piping, CI, docker exec), all output falls
//! back to plain text with no ANSI escape codes and no spinners.

use console::Style;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Returns `true` when stdout is connected to a real terminal.
pub fn is_tty() -> bool {
    console::Term::stdout().is_term()
}

/// Create a cyan spinner with the given message.
///
/// In non-TTY mode returns a hidden progress bar that produces no output.
pub fn spinner(msg: &str) -> ProgressBar {
    if !is_tty() {
        let pb = ProgressBar::hidden();
        pb.set_message(msg.to_string());
        return pb;
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan} {msg}")
            .expect("valid template"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Finish a spinner with a green checkmark and success message.
pub fn step_ok(pb: &ProgressBar, msg: &str) {
    if is_tty() {
        let green = Style::new().green();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("  {msg}")
                .expect("valid template"),
        );
        pb.finish_with_message(format!("{} {msg}", green.apply_to("\u{2713}")));
    } else {
        pb.finish_and_clear();
        println!("  OK: {msg}");
    }
}

/// Finish a spinner with a red cross and failure message.
pub fn step_fail(pb: &ProgressBar, msg: &str) {
    if is_tty() {
        let red = Style::new().red();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("  {msg}")
                .expect("valid template"),
        );
        pb.finish_with_message(format!("{} {msg}", red.apply_to("\u{2717}")));
    } else {
        pb.finish_and_clear();
        eprintln!("  FAIL: {msg}");
    }
}

/// Print a styled key-value line (bold key in TTY, plain otherwise).
pub fn info_line(key: &str, value: &str) {
    if is_tty() {
        let bold = Style::new().bold();
        println!("  {}: {value}", bold.apply_to(key));
    } else {
        println!("  {key}: {value}");
    }
}

/// Print a styled heading.
pub fn heading(text: &str) {
    if is_tty() {
        let bold = Style::new().bold().underlined();
        println!("{}", bold.apply_to(text));
    } else {
        println!("{text}");
        println!("{}", "=".repeat(text.len()));
    }
}

/// Print a green success line (used for final summaries).
pub fn success(msg: &str) {
    if is_tty() {
        let green = Style::new().green().bold();
        println!("{}", green.apply_to(msg));
    } else {
        println!("{msg}");
    }
}

/// Prompt the user for y/n confirmation. Returns `true` if they accept.
///
/// In non-TTY mode always returns `false` (non-interactive defaults to no).
/// Callers that need unattended confirmation should use an explicit flag
/// (e.g. `--force`) instead of relying on the prompt.
pub fn confirm(prompt: &str) -> bool {
    use std::io::Write;
    if !is_tty() {
        return false;
    }
    let yellow = Style::new().yellow();
    eprint!("{} [y/N] ", yellow.apply_to(prompt));
    let _ = std::io::stderr().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Print a yellow warning line.
pub fn warn(msg: &str) {
    if is_tty() {
        let yellow = Style::new().yellow();
        eprintln!("{}", yellow.apply_to(msg));
    } else {
        eprintln!("WARNING: {msg}");
    }
}

/// Print a styled pass/fail check line for diagnostics.
pub fn check_pass(name: &str) {
    if is_tty() {
        let green = Style::new().green();
        println!("  {} {name}", green.apply_to("\u{2713}"));
    } else {
        println!("  [PASS] {name}");
    }
}

/// Print a styled fail check line for diagnostics.
pub fn check_fail(name: &str, detail: &str) {
    if is_tty() {
        let red = Style::new().red();
        println!("  {} {name}: {detail}", red.apply_to("\u{2717}"));
    } else {
        println!("  [FAIL] {name}: {detail}");
    }
}

/// Print a styled join request card for peering watch.
pub fn join_request_card(node_name: &str, endpoint: &str, wg_key_prefix: &str) {
    if is_tty() {
        let cyan = Style::new().cyan();
        let bold = Style::new().bold();
        println!(
            "\n  {} Join request from {}",
            cyan.apply_to("\u{250c}\u{2500}"),
            bold.apply_to(node_name)
        );
        println!("  {}  Endpoint: {endpoint}", cyan.apply_to("\u{2502}"));
        println!("  {}  WG key:   {wg_key_prefix}", cyan.apply_to("\u{2502}"));
        print!("  {} Accept? [Y/n] ", cyan.apply_to("\u{2514}\u{2500}"));
    } else {
        println!("\nJoin request from {node_name} ({endpoint})");
        println!("  WG pubkey: {wg_key_prefix}");
        print!("  Accept? [Y/n] ");
    }
}

/// Print a peering-active banner.
///
/// When `continuous` is true (`--watch` mode), shows "Press Ctrl+C to stop."
/// In default mode, just says "Waiting for join request..." since it exits
/// after the first accept/reject.
pub fn peering_banner(port: u16, pin: Option<&str>, continuous: bool) {
    if is_tty() {
        let green = Style::new().green();
        println!(
            "  {} Peering active on port {port}",
            green.apply_to("\u{2713}")
        );
        if let Some(p) = pin {
            println!("  Mode: auto-accept with PIN");
            println!("  Nodes can join with: syfrah fabric join <this-ip> --pin {p}");
        } else if continuous {
            println!("  Mode: manual approval (you will be prompted for each join request)");
            println!("  Press Ctrl+C to stop.");
        } else {
            println!("  Waiting for join request...");
        }
        println!();
    } else {
        println!("Peering active on port {port}");
        if let Some(p) = pin {
            println!("Mode: auto-accept with PIN");
            println!("Nodes can join with: syfrah fabric join <this-ip> --pin {p}");
        } else if continuous {
            println!("Mode: manual approval");
            println!("Press Ctrl+C to stop.");
        } else {
            println!("Waiting for join request...");
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_returns_progress_bar() {
        // In test (non-TTY), should return hidden bar
        let pb = spinner("test message");
        step_ok(&pb, "done");
    }

    #[test]
    fn step_fail_does_not_panic() {
        let pb = spinner("failing");
        step_fail(&pb, "something broke");
    }

    #[test]
    fn info_line_does_not_panic() {
        info_line("Key", "Value");
    }

    #[test]
    fn heading_does_not_panic() {
        heading("Test Heading");
    }

    #[test]
    fn check_pass_fail_do_not_panic() {
        check_pass("something works");
        check_fail("something broke", "details here");
    }
}
