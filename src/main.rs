use std::env;
use std::process;

use web_migration::Manager;

fn main() {
    let arguments: Vec<String> = env::args().skip(1).collect();
    match &arguments[..] {
        [a] => match Manager::run(a) {
            Ok(report) => {
                println!("Application completed successfully");
                println!("Report Location: {report}");
            
                process::exit(0);
            },
            Err(e) => {
                println!("Application error: {e}");
    
                process::exit(1);
            },
        },
        _ => {
            println!("Invalid amount of arguments");

            process::exit(1);
        }
    }
}
