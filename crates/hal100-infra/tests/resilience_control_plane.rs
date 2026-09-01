#[path = "common/mod.rs"]
mod common;

#[tokio::test]
async fn shared_control_plane_resilience_contract_passes() {
    let evidence = common::verify_control_plane_resilience().await;
    assert!(evidence.all_passed());
}
