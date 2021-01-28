use kube::{Client};
use crate::errors::Error;

pub fn run(_client: Client) -> Result<(), Error> {
    Ok(())
}