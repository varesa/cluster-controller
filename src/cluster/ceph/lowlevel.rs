use librados_sys::{
    rados_t,
    rados_ioctx_t,
    rados_create,
    rados_conf_set,
    rados_conf_read_file,
    rados_connect,
    rados_ioctx_create,
    rados_ioctx_destroy,
    rados_pool_list,
    rados_shutdown,
};

use librbd_sys::{
    rbd_list,
    rbd_create,
};

use crate::errors::{RadosError, Error};
use std::ffi::CString;

macro_rules! call {
    ($operation:literal, $rados_call:expr) => {
        let code = $rados_call;
        if code < 0 {
            return Err(RadosError { operation: String::from($operation), code}.into());
        }
    }
}

pub fn connect() -> Result<rados_t, Error> {
    unsafe {
        let mut cluster: rados_t = 0 as rados_t;

        // Must be returned and freed at the end
        let user_c = CString::new("admin")
            .expect("Failed to create CString").into_raw();
        let opt_keyring_c = CString::new("keyring")
            .expect("Failed to create CString").into_raw();
        let keyring_c = CString::new("/etc/ceph/ceph.client.admin.keyring")
            .expect("Failed to create CString").into_raw();
        let conf_c = CString::new("/etc/ceph/ceph.conf")
            .expect("Failed to create CString").into_raw();

        call!("rados_create", rados_create(&mut cluster, user_c));
        call!("rados_conf_read_file", rados_conf_read_file(cluster, conf_c));
        call!("rados_conf_set", rados_conf_set(cluster, opt_keyring_c, keyring_c));
        call!("rados_conect", rados_connect(cluster));

        // Return control to rust for freeing the memory
        CString::from_raw(user_c);
        CString::from_raw(opt_keyring_c);
        CString::from_raw(keyring_c);
        CString::from_raw(conf_c);
        Ok(cluster)
    }
}

pub fn disconnect(cluster: rados_t) -> () {
    unsafe {
        rados_shutdown(cluster);
    }
}

fn null_separeted_to_vec(null_separated_list: Vec<u8>) -> Vec<String> {
    let mut result = Vec::new();
    for item in null_separated_list.split(|c| { *c == 0 }) {
        if item.len() == 0 { break; }
        result.push(String::from_utf8(item.into())
            .expect("Failed to convert pool name to String"));
    }
    result
}

#[allow(dead_code)]
pub fn get_pools(cluster: rados_t) -> Result<Vec<String>, Error> {
    let mut buffer = vec![0 as u8; 1024];
    let buffer_len = buffer.len();
    unsafe {
        call!("rados_pool_list", rados_pool_list(cluster, buffer.as_mut_ptr() as *mut i8, buffer_len));
    }
    let pools = null_separeted_to_vec(buffer);
    Ok(pools)
}

pub fn get_pool(cluster: rados_t, pool_name: String) -> Result<rados_ioctx_t, Error> {
    let mut pool: rados_ioctx_t = 0 as rados_ioctx_t;

    unsafe {
        // Must be returned and freed at the end
        let pool_name_c = CString::new(pool_name)
            .expect("Failed to create CString").into_raw();

        call!("rados_ioctx_create", rados_ioctx_create(cluster, pool_name_c, &mut pool));

        // Take back control to free memory
        CString::from_raw(pool_name_c);
    }
    Ok(pool)
}

pub fn close_pool(pool: rados_ioctx_t) -> () {
    unsafe {
        rados_ioctx_destroy(pool);
    }
}

pub fn get_images(pool: rados_ioctx_t) -> Result<Vec<String>, Error> {
    let mut buffer = vec![0 as u8; 1024];
    let mut buffer_len: libc::size_t = buffer.len();

    unsafe {
        call!("rbd_list", rbd_list(pool, buffer.as_mut_ptr() as *mut i8, &mut buffer_len));
    }

    let images = null_separeted_to_vec(buffer);
    Ok(images)
}

pub fn create_image(pool: rados_ioctx_t, name: String, size: u64) -> Result<(), Error> {
    unsafe {
        let name_c = CString::new(name)
            .expect("failed to convert to cstring")
            .into_raw();
        call!("rbd_create", rbd_create(pool, name_c, size, &mut 0));

        // Take back control and release memory
        CString::from_raw(name_c);
    }
    Ok(())
}