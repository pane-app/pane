use std::{collections::HashMap, env, fs, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = env::args().nth(1).unwrap_or_else(|| "success".into());
    if mode == "denied" {
        assert!(fs::read_to_string("fixture.txt").is_err());
        println!("denied-ok");
        return Ok(());
    }
    assert_eq!(env::var("P3_PROBE")?, "runtime-value");
    let fixture = fs::read_to_string("fixture.txt")?;
    assert_eq!(fixture, "p3 standard library fixture\n");
    let mut map = HashMap::new(); // Exercises std's random HashMap seed.
    map.insert("fixture", fixture.clone());
    assert_eq!(map.get("fixture"), Some(&fixture));
    fs::write("roundtrip.txt", fixture.as_bytes())?;
    assert_eq!(fs::read_to_string("roundtrip.txt")?, fixture);
    fs::remove_file("roundtrip.txt")?;
    let clock = Instant::now();
    std::thread::sleep(Duration::from_millis(10));
    assert!(clock.elapsed() >= Duration::from_millis(10));
    let epoch_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    assert!(epoch_ms > 1_700_000_000_000);
    eprintln!("p3 diagnostics work");
    println!("{{\"ok\":true,\"epochMs\":{epoch_ms},\"entries\":{}}}", map.len());
    Ok(())
}
