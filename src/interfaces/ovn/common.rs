use std::sync::Arc;

use serde_json::{Map, Value};
use tracing::{info, instrument};

use crate::Error;
use crate::interfaces::ovn::lowlevel::Ovn;

/// Base properties that are required by most OVN methods
pub trait OvnCommon: Sized {
    fn uuid(&self) -> String;
    fn ovn(&self) -> Arc<Ovn>;
    fn ovn_type() -> String;
    fn deserialize(ovn: Arc<Ovn>, value: &Value) -> Result<Self, Error>;
}

/// Represents an OVN type that has a name property (e.g. most of them)
pub trait OvnNamed: OvnCommon {
    fn name(&self) -> String;
}

/// Contains UUID-based getters
pub trait OvnGetters: Sized {
    fn list(ovn: Arc<Ovn>) -> Result<Vec<Self>, Error>;
    //fn get_by_uuid(ovn: &Ovn, uuid: &str) -> Result<Self, Error>
}

/// Contains name-based getters
pub trait OvnNamedGetters: Sized {
    fn get_by_name(ovn: Arc<Ovn>, name: &str) -> Result<Self, Error>;
}

/// A meta-trait for OVN types that can be created with only the name and no extra information
pub trait OvnBasicType: OvnNamed {}

/// Trait containing method implementations for OvnBasicType
pub trait OvnBasicActions: OvnBasicType {
    fn create(ovn: Arc<Ovn>, name: &str) -> Result<Self, Error>;
    fn create_if_missing(ovn: Arc<Ovn>, name: &str) -> Result<Self, Error>;
    fn delete(self) -> Result<(), Error>;
}

impl<T> OvnGetters for T
where
    T: OvnCommon,
{
    /*fn get_by_uuid(ovn: &Ovn, uuid: &str) -> Result<LogicalSwitch, Error> {
        unimplemented!()
    }*/

    fn list(ovn: Arc<Ovn>) -> Result<Vec<Self>, Error> {
        ovn.list_objects(&Self::ovn_type())
            .iter()
            .map(|o| Self::deserialize(ovn.clone(), o))
            .collect()
    }
}

impl<T> OvnNamedGetters for T
where
    T: OvnNamed,
{
    #[instrument(fields(kind = Self::ovn_type()))]
    fn get_by_name(ovn: Arc<Ovn>, name: &str) -> Result<Self, Error> {
        Self::list(ovn)?
            .into_iter()
            .find(|o| o.name() == name)
            .ok_or_else(|| Error::OvnNotFound(Self::ovn_type(), name.to_string()))
    }
}

impl<T> OvnBasicActions for T
where
    T: OvnBasicType,
{
    fn create(ovn: Arc<Ovn>, name: &str) -> Result<Self, Error> {
        info!("create");
        let mut params = Map::new();
        params.insert("name".to_string(), Value::String(name.to_string()));
        ovn.insert(&Self::ovn_type(), params);

        Self::get_by_name(ovn, name)
    }

    #[instrument]
    fn create_if_missing(ovn: Arc<Ovn>, name: &str) -> Result<Self, Error> {
        info!("create_if_missing");
        match Self::get_by_name(ovn.clone(), name) {
            Ok(ls) => Ok(ls),
            Err(Error::OvnNotFound(_, _)) => {
                info!("ovn: {} {} doesn't exist, creating", Self::ovn_type(), name);
                Self::create(ovn, name)
            }
            Err(e) => Err(e),
        }
    }

    #[allow(unused_mut)]
    fn delete(mut self) -> Result<(), Error> {
        self.ovn().delete_by_uuid(&Self::ovn_type(), &self.uuid());
        Ok(())
    }
}
