use std::time::Duration;

use voltrs::prelude::*;

#[model]
struct Report {
    title: String,
}

/// A slow data source: client navigations to `/slow` show `app/slow/loading.tsx` meanwhile.
#[loader("/slow")]
async fn slow(_ctx: Ctx) -> Result<Report> {
    tokio::time::sleep(Duration::from_millis(1500)).await;
    Ok(Report { title: "Slow report".into() })
}

fn main() {
    App::new().run();
}
