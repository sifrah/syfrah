use crate::daemon;
use crate::ui;
use anyhow::Result;

pub async fn run() -> Result<()> {
    let sp = ui::spinner("Leaving mesh...");
    match daemon::run_leave().await {
        Ok(()) => {
            ui::step_ok(&sp, "Left the mesh. State cleared.");
            Ok(())
        }
        Err(e) => {
            ui::step_fail(&sp, &format!("Failed to leave mesh: {e}"));
            Err(e)
        }
    }
}
