use super::{ControlPlane, TestResult};

pub(super) async fn install(plane: &mut ControlPlane) -> TestResult {
    let client = plane.client();
    plane
        .checked("installing VirtualMachine CRD", async {
            crate::crd::virtualmachine::create(client).await?;
            Ok(())
        })
        .await
}
