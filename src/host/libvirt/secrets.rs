use crate::host::libvirt::lowlevel::Libvirt;
use crate::host::libvirt::templates::SecretTemplate;
use crate::{Client, Error, KEYRING_SECRET, NAMESPACE};
use askama::Template;
use k8s_openapi::api::core::v1::Secret;
use kube::Api;
use virt::secret::Secret as LibvirtSecret;

const CEPH_SECRET_UUID: &str = "8e22b0ac-b429-4ad1-8783-6d792db31349";

fn create_secret(key: &[u8], libvirt: &Libvirt) -> Result<(), Error> {
    let xml = SecretTemplate {
        uuid: CEPH_SECRET_UUID.into(),
        name: "client.libvirt secret".into(),
        usage: "ceph".into(),
    }
    .render()?;

    let secret = LibvirtSecret::define_xml(&libvirt.connection, &xml, 0)?;
    secret.set_value(key, 0)?;

    Ok(())
}

pub async fn ensure_ceph_secret(kube: Client, libvirt: &Libvirt) -> Result<(), Error> {
    if LibvirtSecret::lookup_by_uuid_string(&libvirt.connection, CEPH_SECRET_UUID).is_ok() {
        println!("Secret found");
        return Ok(());
    }
    println!("Secret missing");

    let secrets: Api<Secret> = Api::namespaced(kube.clone(), NAMESPACE);
    let secret = match secrets.get(KEYRING_SECRET).await {
        Err(e) => return Err(e.into()),
        Ok(secret) => secret,
    };

    let data = secret.data.unwrap();
    let key = data.get("key").unwrap().0.clone();
    create_secret(key.as_ref(), libvirt)?;
    println!("Secret created");
    Ok(())
}
