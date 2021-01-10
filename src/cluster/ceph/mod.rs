mod lowlevel;

use crate::errors::Error;

pub async fn run() -> Result<(), Error> {
    let mut cluster = lowlevel::connect()?;
    lowlevel::list_pools(cluster)?;
    Ok(())
}