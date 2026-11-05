//! Imports one private secret-prompt file. Prints no secret; never accepts tokens as arguments.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let key = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected credential key and private file path"))?;
    let path = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected private file path"))?;
    let mut secret = std::fs::read(&path)?;
    let result = std::str::from_utf8(&secret)
        .map_err(anyhow::Error::from)
        .and_then(|value| mypaper_integrations::credentials::write(&key, value.trim()));
    secret.fill(0);
    result?;
    println!("Stored {key} in Windows Credential Manager");
    Ok(())
}
