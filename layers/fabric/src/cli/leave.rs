use crate::daemon;
use crate::ui;
use anyhow::Result;

pub async fn run() -> Result<()> {
    if !ui::confirm("Leave the current mesh? This will remove all peer connections.") {
        eprintln!("Aborted.");
        return Ok(());
    }

    let sp = ui::spinner("Leaving mesh...");
    match daemon::run_leave().await {
        Ok(true) => {
            // run_leave already printed detailed progress; just finish the
            // outer spinner so the CLI output stays tidy.
            sp.finish_and_clear();
            Ok(())
        }
        Ok(false) => {
            ui::step_ok(&sp, "No mesh configured. Nothing to do.");
            Ok(())
        }
        Err(e) => {
            ui::step_fail(&sp, &format!("Failed to leave mesh: {e}"));
            Err(e)
        }
    }
}
