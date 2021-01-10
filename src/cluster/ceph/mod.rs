mod lowlevel;

use crate::errors::Error;

pub async fn run() -> Result<(), Error> {
    lowlevel::connect()?;
    Ok(())
}