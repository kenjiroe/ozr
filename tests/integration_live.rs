use ozr::core::config::AppConfig;
use ozr::core::integration_fixtures::{
    integration_enabled, run_qdrant_fixture, run_sandboxd_fixture,
};

fn require_integration() {
    if !integration_enabled() {
        panic!("set OZR_RUN_INTEGRATION=1 to run live integration fixtures");
    }
}

#[test]
#[ignore = "requires live sandboxd endpoint (OZR_RUN_INTEGRATION=1)"]
fn sandboxd_live_fixture() {
    require_integration();
    let cfg = AppConfig::from_env();
    let report = run_sandboxd_fixture(&cfg).expect("sandboxd fixture failed");
    assert_eq!(report.name, "sandboxd");
    eprintln!("sandboxd fixture ok: {}", report.detail);
}

#[test]
#[ignore = "requires live qdrant endpoint (OZR_RUN_INTEGRATION=1)"]
fn qdrant_live_fixture() {
    require_integration();
    let cfg = AppConfig::from_env();
    let report = run_qdrant_fixture(&cfg).expect("qdrant fixture failed");
    assert_eq!(report.name, "qdrant");
    eprintln!("qdrant fixture ok: {}", report.detail);
}
