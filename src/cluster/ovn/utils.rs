use crate::cluster::ovn::common::OvnNamed;
use crate::cluster::ovn::types::logicalrouter::LogicalRouter;
use crate::cluster::ovn::types::logicalswitch::LogicalSwitch;
use crate::errors::Error;
use serde_json::json;

pub fn connect_router_to_ls(
    router: &mut LogicalRouter,
    switch: &mut LogicalSwitch,
    address: &str,
) -> Result<(), Error> {
    let lrp_name = format!("lr_{}_ls_{}", router.name(), switch.name());

    router
        .lrp()
        .create_if_missing(&lrp_name, address)?
        // TODO: do something about redundant update if LRP already exists
        .update(address)?;

    let lsp_name = format!("ls_{}_lr_{}", switch.name(), router.name());
    let params = json!({
        "type": "router",
        "addresses": "router",
        "options": ["map", [ ["router-port", lrp_name] ]]
    });
    switch
        .lsp()
        .create_if_missing(&lsp_name, Some(params.as_object().unwrap()))?;
    Ok(())
}
