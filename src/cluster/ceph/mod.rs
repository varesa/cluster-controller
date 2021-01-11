mod lowlevel;

use crate::errors::Error;

pub async fn run() -> Result<(), Error> {
    let mut cluster = lowlevel::connect()?;
    lowlevel::list_pools(cluster)?;
    let mut pool = lowlevel::get_pool(cluster, "volumes".into())?;
    lowlevel::list_images(pool)?;
    Ok(())
}