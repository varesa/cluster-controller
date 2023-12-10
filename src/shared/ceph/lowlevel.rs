use libc::{c_char, c_int, ERANGE};
use serde_json::json;
use std::{ffi::CString, ptr, str};

use librados_sys::{
    rados_buffer_free, rados_conf_read_file, rados_conf_set, rados_connect, rados_create,
    rados_ioctx_create, rados_ioctx_destroy, rados_ioctx_t, rados_mon_command, rados_pool_list,
    rados_shutdown, rados_t,
};

use librbd_sys::{
    rbd_clone, rbd_close, rbd_create, rbd_get_features, rbd_image_t, rbd_list, rbd_list_lockers,
    rbd_open, rbd_open_read_only, rbd_remove,
};
use tracing::instrument;

use crate::errors::{Error, RadosError};

macro_rules! call {
    ($operation:literal, $rados_call:expr) => {
        let code = $rados_call;
        if code < 0 {
            return Err(RadosError {
                operation: String::from($operation),
                code,
            }
            .into());
        }
    };
}

macro_rules! create_cstring {
    ([
        $(($name:ident, $value:expr)),+
    ]) => {
        $(
            let $name = CString::new($value).expect(&format!("Failed to create CString {}", stringify!($name))).into_raw();
        )+
    };
}

macro_rules! drop_cstring {
    ([$($name:ident),+]) => {
        $(
        drop(CString::from_raw($name));
        )+
    }
}

#[instrument]
pub fn connect() -> Result<rados_t, Error> {
    unsafe {
        let mut cluster: rados_t = 0 as rados_t;

        // Must be returned and freed at the end
        create_cstring!([
            (user_c, "admin"),
            (opt_keyring_c, "keyring"),
            (keyring_c, "/etc/ceph/ceph.client.admin.keyring"),
            (conf_c, "/etc/ceph/ceph.conf")
        ]);

        call!("rados_create", rados_create(&mut cluster, user_c));
        call!(
            "rados_conf_read_file",
            rados_conf_read_file(cluster, conf_c)
        );
        call!(
            "rados_conf_set",
            rados_conf_set(cluster, opt_keyring_c, keyring_c)
        );
        call!("rados_conect", rados_connect(cluster));

        drop_cstring!([user_c, opt_keyring_c, keyring_c, conf_c]);
        Ok(cluster)
    }
}

#[instrument]
pub fn disconnect(cluster: rados_t) {
    unsafe {
        rados_shutdown(cluster);
    }
}

fn null_separated_to_vec(null_separated_list: Vec<u8>) -> Vec<String> {
    let mut result = Vec::new();
    for item in null_separated_list.split(|c| *c == 0) {
        if item.is_empty() {
            break;
        }
        result.push(String::from_utf8(item.into()).expect("Failed to convert pool name to String"));
    }
    result
}

#[allow(dead_code)]
pub fn get_pools(cluster: rados_t) -> Result<Vec<String>, Error> {
    let mut buffer = vec![0u8; 1024];
    let buffer_len = buffer.len();
    unsafe {
        call!(
            "rados_pool_list",
            rados_pool_list(cluster, buffer.as_mut_ptr() as *mut i8, buffer_len)
        );
    }
    let pools = null_separated_to_vec(buffer);
    Ok(pools)
}

#[instrument]
pub fn get_pool(cluster: rados_t, pool_name: String) -> Result<rados_ioctx_t, Error> {
    let mut pool: rados_ioctx_t = 0 as rados_ioctx_t;

    unsafe {
        // Must be returned and freed at the end
        let pool_name_c = CString::new(pool_name)
            .expect("Failed to create CString")
            .into_raw();

        call!(
            "rados_ioctx_create",
            rados_ioctx_create(cluster, pool_name_c, &mut pool)
        );

        // Take back control to free memory
        drop(CString::from_raw(pool_name_c));
    }
    Ok(pool)
}

#[instrument]
pub fn close_pool(pool: rados_ioctx_t) {
    unsafe {
        rados_ioctx_destroy(pool);
    }
}

#[instrument]
pub fn get_images(pool: rados_ioctx_t) -> Result<Vec<String>, Error> {
    let mut buffer = vec![0u8; 1024];
    let mut buffer_len: libc::size_t = buffer.len();

    unsafe {
        call!(
            "rbd_list",
            rbd_list(pool, buffer.as_mut_ptr() as *mut i8, &mut buffer_len)
        );
    }

    let images = null_separated_to_vec(buffer);
    Ok(images)
}

pub fn create_image(pool: rados_ioctx_t, name: &str, size: u64) -> Result<(), Error> {
    unsafe {
        let name_c = CString::new(name)
            .expect("failed to convert to cstring")
            .into_raw();
        call!("rbd_create", rbd_create(pool, name_c, size, &mut 0));

        // Take back control and release memory
        drop(CString::from_raw(name_c));
    }
    Ok(())
}

pub fn get_features(pool: rados_ioctx_t, name: &str, snapshot: &str) -> Result<u64, Error> {
    let mut features: u64 = 0;
    unsafe {
        create_cstring!([(name_c, name), (snapshot_c, snapshot)]);

        let mut image: rbd_image_t = 0 as rbd_image_t;
        call!("rbd_open", rbd_open(pool, name_c, &mut image, snapshot_c));

        call!("rbd_get_features", rbd_get_features(image, &mut features));

        call!("rbd_close", rbd_close(image));
        drop_cstring!([name_c, snapshot_c]);
    }

    Ok(features)
}

pub fn clone_image(
    pool: rados_ioctx_t,
    name: &str,
    _size: u64,
    template_pool: rados_ioctx_t,
    template_name: &str,
) -> Result<(), Error> {
    unsafe {
        create_cstring!([
            (name_c, name),
            (template_name_c, template_name),
            (snapshot_name_c, "default")
        ]);

        let features = get_features(template_pool, template_name, "default")?;

        call!(
            "rbd_clone",
            rbd_clone(
                template_pool,
                template_name_c,
                snapshot_name_c,
                pool,
                name_c,
                features,
                &mut 0
            )
        );
        drop_cstring!([name_c, template_name_c, snapshot_name_c]);
    }
    Ok(())
}

pub fn remove_image(pool: rados_ioctx_t, name: &str) -> Result<(), Error> {
    unsafe {
        let name_c = CString::new(name)
            .expect("failed to convert to cstring")
            .into_raw();
        call!("rbd_delete", rbd_remove(pool, name_c));

        drop(CString::from_raw(name_c));
    }
    Ok(())
}

pub fn auth_get_key(cluster: rados_t, key_name: String) -> Result<String, Error> {
    let cmd = json!({
        "prefix": "auth get-key",
        "entity": key_name
    })
    .to_string();

    // Important! The .as[_mut]_ptr() must not be combined with the previous line,
    // or the "intermediate" product will be dropped and the pointer will become
    // invalid
    let cmd_cstr = CString::new(cmd).unwrap();
    let cmd_ptr = cmd_cstr.as_ptr();
    let mut cmd_array = vec![cmd_ptr];
    let cmd_array_ptr = cmd_array.as_mut_ptr();

    let mut outbuf = ptr::null_mut();
    let mut outs = ptr::null_mut();
    let mut outbuf_len = 0;
    let mut outs_len = 0;

    let mut key: Option<String> = None;

    unsafe {
        call!(
            "rados_mon_command (auth get-key)",
            rados_mon_command(
                cluster,
                /* command */
                cmd_array_ptr,
                1,
                /* input data */
                ptr::null_mut::<c_char>(),
                0,
                /* output data */
                &mut outbuf,
                &mut outbuf_len,
                /* other outputs */
                &mut outs,
                &mut outs_len,
            )
        );

        if outbuf_len > 0 {
            let key_bytes = std::slice::from_raw_parts(outbuf as *const u8, outbuf_len);
            key = Some(
                str::from_utf8(key_bytes)
                    .expect("Failed to decode key")
                    .to_owned(),
            );
            rados_buffer_free(outbuf);
        }
        if outs_len > 0 {
            rados_buffer_free(outs);
        }
    }
    match key {
        Some(key) => Ok(key),
        None => Err(RadosError {
            operation: String::from("auth get-key"),
            code: 1,
        }
        .into()),
    }
}

fn open_image(pool: rados_ioctx_t, image_name: &str) -> Result<rbd_image_t, Error> {
    let mut image: rbd_image_t = 0 as rbd_image_t;

    unsafe {
        // Must be returned and freed at the end
        let image_name_c = CString::new(image_name)
            .expect("Failed to create CString")
            .into_raw();

        call!(
            "rbd_open_read_only",
            rbd_open_read_only(pool, image_name_c, &mut image, std::ptr::null())
        );

        // Take back control to free memory
        drop(CString::from_raw(image_name_c));
    }
    Ok(image)
}

pub fn has_locks(pool: rados_ioctx_t, image_name: &str) -> Result<bool, Error> {
    let image = open_image(pool, image_name)?;

    let mut is_exclusive = 0;
    let mut tag_len = 0;
    let mut clients_len = 0;
    let mut cookies_len = 0;
    let mut addrs_len = 0;

    /*
    We can get away with supplying null buffers, if we also give 0 as the buffer length.
    rbd_list_lockers will check if the data will fit in the buffers (it will not if there are any
    locks) and return -ERANGE if it would not.
     */

    let code = unsafe {
        rbd_list_lockers(
            image,
            &mut is_exclusive,
            /* tag */
            std::ptr::null_mut(),
            &mut tag_len,
            /* clients */
            std::ptr::null_mut(),
            &mut clients_len,
            /* cookies */
            std::ptr::null_mut(),
            &mut cookies_len,
            /* addresses */
            std::ptr::null_mut(),
            &mut addrs_len,
        )
    } as i32;

    const NEG_ERANGE: c_int = -ERANGE;

    match code {
        0 => Ok(false),
        NEG_ERANGE => {
            let locks = clients_len > 0 || cookies_len > 0 || addrs_len > 0;
            Ok(locks)
        }
        _ => Err(RadosError {
            operation: String::from("rbd_list_lockers"),
            code,
        }
        .into()),
    }
}
