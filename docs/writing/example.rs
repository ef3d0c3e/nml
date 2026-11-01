use std::io;

fn main() {
    // nml:begin greet
    let name = "NML";
    if !name.is_empty() {
        println!("Hello, {name}!");
    }
    // nml:end greet
}
