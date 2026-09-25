use super::ControlPlane;
use crate::errors::Error;

pub(super) async fn install(plane: &mut ControlPlane) -> Result<(), Error> {
    let client = plane.client();
    crate::crd::cluster::create(client.clone()).await?;
    crate::crd::libvirtnode::create(client.clone()).await?;
    crate::crd::virtualmachine::create(client.clone()).await?;
    crate::crd::network::create(client.clone()).await?;
    Ok(())
}
