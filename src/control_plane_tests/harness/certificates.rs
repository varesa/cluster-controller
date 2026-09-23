use std::fs;
use std::path::Path;
use std::process::Command;

pub fn generate(path: &Path) -> Result<(), String> {
    fs::write(
        path.join("server.ext"),
        "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1,DNS:localhost\n",
    ).map_err(|err| err.to_string())?;
    fs::write(
        path.join("admin.ext"),
        "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n",
    ).map_err(|err| err.to_string())?;
    let commands: &[&[&str]] = &[
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=control-plane-ca",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-keyout",
            "ca.key",
            "-out",
            "ca.crt",
        ],
        &["x509", "-in", "ca.crt", "-outform", "DER", "-out", "ca.der"],
        &[
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-keyout",
            "server.key",
            "-out",
            "server.csr",
        ],
        &[
            "x509",
            "-req",
            "-in",
            "server.csr",
            "-CA",
            "ca.crt",
            "-CAkey",
            "ca.key",
            "-set_serial",
            "2",
            "-days",
            "1",
            "-extfile",
            "server.ext",
            "-out",
            "server.crt",
        ],
        &[
            "req",
            "-new",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=control-plane-admin/O=system:masters",
            "-keyout",
            "admin.key",
            "-out",
            "admin.csr",
        ],
        &[
            "x509",
            "-req",
            "-in",
            "admin.csr",
            "-CA",
            "ca.crt",
            "-CAkey",
            "ca.key",
            "-set_serial",
            "3",
            "-days",
            "1",
            "-extfile",
            "admin.ext",
            "-out",
            "admin.crt",
        ],
        &["genrsa", "-out", "service-account.key", "2048"],
    ];
    for args in commands {
        let mut process = Command::new("openssl")
            .current_dir(path)
            .args(*args)
            .spawn()
            .map_err(|e| {
                format!(
                    "spawn: openssl {} failed generating certificates: {e}",
                    args[0]
                )
            })?;
        let status = process.wait().map_err(|e| {
            format!(
                "wait: openssl {} failed generating certificates: {e}",
                args[0]
            )
        })?;

        if !status.success() {
            return Err(format!(
                "result: openssl {} failed generating certificates: {status}",
                args[0]
            ));
        }
    }
    Ok(())
}
