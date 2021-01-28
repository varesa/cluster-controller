//mod lowlevel;
mod crd;

use kube::Client;

use crate::errors::Error;

pub async fn run(client: Client) -> Result<(), Error> {
    /*let mut cluster = lowlevel::connect()?;

    let pools = lowlevel::get_pools(cluster)?;
    println!("Pools: {:?}", pools);

    let mut pool = lowlevel::get_pool(cluster, "volumes".into())?;

    let images = lowlevel::get_images(pool)?;
    println!("Images: {:?}", images);*/
    crd::create(client).await?;
    Ok(())
}