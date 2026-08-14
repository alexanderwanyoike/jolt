use std::collections::BTreeMap;

use axum::Json;
use serde::Serialize;

const APP_API_LEVEL: u32 = 1;

#[derive(Debug, Serialize)]
pub struct AppApiFeatureManifest {
    app_api: u32,
    features: BTreeMap<&'static str, u32>,
}

pub async fn get_features() -> Json<AppApiFeatureManifest> {
    Json(AppApiFeatureManifest {
        app_api: APP_API_LEVEL,
        features: BTreeMap::new(),
    })
}
