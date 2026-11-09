//! Runs the showcase on the desktop, on its sample documents: `mise run showcase`.

use showcase::app::App;
use showcase::pool::Pool;
use showcase::service::Service;

fn main() -> Result<(), slint::PlatformError> {
    let pool = Pool::new(4);
    let app = App::start(Service::new(showcase::SAMPLE), pool.background());
    // Runs until the last window closes.
    slint::run_event_loop()?;
    drop(app);
    // Last, so that work already on the pool finishes.
    drop(pool);
    Ok(())
}
