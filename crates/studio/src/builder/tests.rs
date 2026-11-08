use super::*;

#[test]
fn contract_is_validated_by_the_engine() {
    validate_theme(r##"{"name":"Test","wallpaper":{"kind":"stack","base":{"kind":"color","color":"#202124"},"layers":[]}}"##).unwrap();
    assert!(validate_theme(r##"{"name":"Test","wallpaper":{"kind":"stack","scale":2,"base":{"kind":"color","color":"#202124"},"layers":[]}}"##).is_err());
}
