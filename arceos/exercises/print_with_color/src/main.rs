#![cfg_attr(feature = "axstd", no_std)]
#![cfg_attr(feature = "axstd", no_main)]

#[cfg(feature = "axstd")]
use axstd::println;

#[cfg_attr(feature = "axstd", no_mangle)]
fn main() {
    println!("\x1b[31mred!\x1b[0m");
    println!("\x1b[32mgreen!\x1b[0m");
    println!("\x1b[34mblue!\x1b[0m");

    println!("\x1b[31m[red]: Hello, Arceos!\x1b[0m");
}
