use std::collections::BTreeMap;

use axum::Json;
use serde::Serialize;

// Higher levels are strictly additive compatibility floors. Breaking App API
// behavior requires a parallel `/app/vN` route rather than incrementing this.
const APP_API_LEVEL: u32 = 1;

#[derive(Debug, Serialize)]
pub struct AppApiFeatureManifest {
    app_api: u32,
    features: BTreeMap<&'static str, u32>,
}

pub async fn get_features() -> Json<AppApiFeatureManifest> {
    Json(AppApiFeatureManifest {
        app_api: APP_API_LEVEL,
        features: BTreeMap::from([
            ("data.change-streams", 1),
            ("data.records", 5),
            ("data.subscriptions", 1),
        ]),
    })
}
