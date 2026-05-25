//! ASCII banner shown at strategic moments (e.g. `re init`).
//!
//! The art is a stylized rendering of the Causari logo — a `C` with a causal
//! graph passing through it, ending in an arrow. The dots and the arrow on
//! the path encode the product itself: events linked by causality flowing
//! through the codebase.

use colored::Colorize;

const BANNER: &str = r#"
   ___                      _
  / __\__ _ _   _ ___  __ _| |__(_)
 / /  / _` | | | / __|/ _` | '__| |
/ /__| (_| | |_| \__ \ (_| | |  | |
\____/\__,_|\__,_|___/\__,_|_|  |_|

      o━━━o━━━o━━━o━━▶
     A   B   C   D   …
"#;

/// Print the colored banner with both taglines.
pub fn print_banner() {
    // Color the banner cyan-ish so it pops in dark terminals without being loud.
    for line in BANNER.lines() {
        println!("{}", line.bright_cyan().bold());
    }
    println!(
        "  {}  {}",
        "Trace intent.".bright_white().bold(),
        "Debug causality.".bright_magenta().bold()
    );
    println!(
        "  {}",
        "intent-addressable code for AI agents".bright_black()
    );
    println!();
}
