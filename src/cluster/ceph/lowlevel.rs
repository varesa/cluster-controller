use librados_sys::{
    rados_t,
    rados_ioctx_t,
    rados_create,
    rados_conf_set,
    rados_conf_read_file,
    rados_connect,
    rados_ioctx_create,
    rados_pool_list
};

use librbd_sys::{
    rbd_list,
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

pub fn connect() -> Result<rados_t, Error>{
    unsafe {
        let mut cluster: rados_t = 0 as rados_t;

        // Must be returned and freed at the end
        let user_c = CString::new("clusteradmin-dev")
            .expect("Failed to create CString").into_raw();
        let opt_keyring_c = CString::new("keyring")
            .expect("Failed to create CString").into_raw();
        let keyring_c = CString::new("ceph/clusteradmin-dev.key")
            .expect("Failed to create CString").into_raw();
        let conf_c = CString::new("ceph/ceph.conf")
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

pub fn list_pools(cluster: rados_t) -> Result<(), Error> {
    let mut buffer = vec![0 as u8; 1024];
    let buffer_len = buffer.len();
    unsafe {
        let code = rados_pool_list(cluster, buffer.as_mut_ptr() as *mut i8, buffer_len);
    }
    let pools = buffer.split(|c| { *c == 0 });
    for pool in pools {
        if pool.len() == 0 { break; }
        println!("{}", std::str::from_utf8(pool).expect("Failed to convert name to string"));
    }
    Ok(())
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

pub fn list_images(pool: rados_ioctx_t) -> Result<(), Error> {
    let mut buffer = vec![0 as u8; 1024];
    let mut buffer_len: libc::size_t = buffer.len();

    unsafe {
        call!("rbd_list", rbd_list(pool, buffer.as_mut_ptr() as *mut i8, &mut buffer_len));
    }

    let images = buffer.split(|c| { *c == 0});
    for image in images {
        if image.len() == 0 { break; }
        println!("{}", std::str::from_utf8(image).expect("Failed to conver image to string"));
    }
    Ok(())
}