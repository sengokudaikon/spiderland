//! Runs the showcase on the desktop, on its sample documents: `mise run showcase`. It saves where the user left off in the system's
//! temporary folder, and starts there the next time.

use showcase::app::{App, Session};
use showcase::pool::Pool;
use showcase::service::Service;

fn main() -> Result<(), slint::PlatformError> {
    let path = std::env::temp_dir().join("spiderland-showcase-session.json");
    // A session that is missing, or that doesn't read, starts afresh.
    let session: Session = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let pool = Pool::new(4);
    let app = App::restore(Service::new(showcase::SAMPLE), pool.background(), session);
    // Runs until the last window closes.
    slint::run_event_loop()?;
    let session = app.session();
    drop(app);
    // Last, so that work already on the pool finishes.
    drop(pool);
    let written = serde_json::to_string_pretty(&session)
        .map_err(std::io::Error::from)
        .and_then(|text| std::fs::write(&path, text));
    if let Err(error) = written {
        eprintln!("The session wasn't saved to {}: {error}.", path.display());
    }
    Ok(())
}
