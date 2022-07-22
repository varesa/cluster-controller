use crate::Error;

pub mod lowlevel;

pub fn has_locks(pool_name: &str, image_name: &str) -> Result<bool, Error> {
    let cluster = lowlevel::connect()?;
    let pool = lowlevel::get_pool(cluster, pool_name.into())?;

    lowlevel::has_locks(pool, image_name)
}
