use librados_sys::{rados_create, rados_t, rados_conf_set, rados_conf_read_file, rados_connect};
use std::ptr::null;

use crate::errors::{RadosError, Error};
use std::ffi::CString;


pub fn connect() -> Result<(), Error>{
    unsafe {
        let mut cluster: rados_t = 0 as rados_t;

        // Free at end
        let user_c = CString::new("clusteradmin-dev")
            .expect("Failed to create CString").into_raw();
        let opt_keyring_c = CString::new("keyring")
            .expect("Failed to create CString").into_raw();
        let keyring_c = CString::new("ceph/clusteradmin-dev.key")
            .expect("Failed to create CString").into_raw();
        let conf_c = CString::new("ceph/ceph.conf")
            .expect("Failed to create CString").into_raw();

        let code = rados_create(&mut cluster, user_c);
        if code != 0 {
            return Err(RadosError {operation: String::from("rados_create"), code}.into());
        }

        let code = rados_conf_read_file(cluster, conf_c);
        if code != 0 {
            return Err(RadosError {operation: String::from("rados_conf_read_file"), code}.into());
        }

        let code = rados_conf_set(cluster, opt_keyring_c, keyring_c);
        if code != 0 {
            return Err(RadosError {operation: String::from("rados_conf_set keyring"), code}.into());
        }

        let code = rados_connect(cluster);
        if code != 0 {
            return Err(RadosError {operation: String::from("rados_connect"), code}.into());
        }

        // Return control to rust for freeing the memory
        CString::from_raw(user_c);
        CString::from_raw(opt_keyring_c);
        CString::from_raw(keyring_c);
        CString::from_raw(conf_c);
        Ok(())
    }
}