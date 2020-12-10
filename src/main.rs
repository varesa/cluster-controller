mod errors;
mod deployer;
mod host;

use std::env;

use crate::errors::Error;

fn main() -> Result<(), Error>{
    let args: Vec<String> = env::args().collect();
    if args.contains(&String::from("--host")) {
        host::run()?;
    } else {
        deployer::run()?;
    }
    Ok(())
}
