use kube::{Client};
use crate::errors::Error;

pub fn run(client: Client) -> Result<(), Error> {
    Ok(())
}