//! SigmaOS component binary: bi-analytics

use sigmaos_bi-analytics::Component;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Component::new().operate()?;
    println!("sigmaos-bi-analytics: ok");
    Ok(())
}
