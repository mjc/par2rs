/// Test to compare our block detection with par2cmdline
/// Specifically looking at why we're finding fewer blocks in damaged files
use std::process::Command;

#[test]
#[ignore] // Only run manually
fn compare_s07e07_blocks() {
    // Run our implementation
    let output = Command::new("./target/release/par2")
        .args([
            "v",
            "The.Circus.S07.1080p.WEB-DL.H.264-BTN/The.Circus.S07.1080p.WEB-DL.H.264-BTN.par2",
        ])
        .output()
        .expect("Failed to run par2rs");

    let our_output = String::from_utf8_lossy(&output.stdout);
    println!("=== Our output ===");
    println!("{}", our_output);

    // Run par2cmdline
    let output = Command::new("par2")
        .args([
            "v",
            "The.Circus.S07.1080p.WEB-DL.H.264-BTN/The.Circus.S07.1080p.WEB-DL.H.264-BTN.par2",
        ])
        .output()
        .expect("Failed to run par2cmdline");

    let their_output = String::from_utf8_lossy(&output.stdout);
    println!("\n=== par2cmdline output ===");
    println!("{}", their_output);

    // Compare
    println!("\n=== Comparison ===");
    for line in our_output.lines() {
        if line.contains("s07e07") || line.contains("S07E07") {
            println!("Ours: {}", line);
        }
    }
    for line in their_output.lines() {
        if line.contains("s07e07") || line.contains("S07E07") {
            println!("Theirs: {}", line);
        }
    }
}
