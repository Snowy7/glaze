#[test]
fn credential_manager_round_trips_and_removes_a_separate_test_target() {
    let key = format!("test/{}", std::process::id());
    glaze_integrations::credentials::write(&key, "fixture-only").unwrap();
    let read = glaze_integrations::credentials::read(&key).unwrap();
    assert_eq!(read.text().unwrap(), "fixture-only");
    drop(read);
    glaze_integrations::credentials::remove(&key).unwrap();
    assert!(glaze_integrations::credentials::read(&key).is_err());
}
