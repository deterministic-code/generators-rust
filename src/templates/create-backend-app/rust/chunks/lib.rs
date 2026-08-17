// === BEGIN MODULES — see PATCH_PLAN in create-migrate-scripts.mjs ===
// === END MODULES ===

pub fn custom_services() -> deterministic::CustomServices {
    let services = deterministic::CustomServices::new();
    // === BEGIN CUSTOM_SERVICES — see PATCH_PLAN in create-migrate-scripts.mjs ===
    // === END CUSTOM_SERVICES ===
    services
}

pub fn route_composer() -> deterministic::RouteComposer {
    deterministic::RouteComposer::new(crate::routes::generated::app_wiring::compose_router)
}
