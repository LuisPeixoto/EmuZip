//! Autoridade certificadora do EmuZip (gerada uma vez por máquina).
use anyhow::{Context, Result};
use base64::Engine;
use hudsucker::rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use md5::{Digest, Md5};
use std::path::{Path, PathBuf};

pub struct Ca {
    pub cert_pem: String,
    pub key_pem: String,
    pub dir: PathBuf,
}

impl Ca {
    /// Carrega de `dir` ou gera um novo CA (válido por 10 anos).
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let (c, k) = (dir.join("ca.pem"), dir.join("ca.key"));
        if let (Ok(cert_pem), Ok(key_pem)) = (std::fs::read_to_string(&c), std::fs::read_to_string(&k)) {
            return Ok(Self { cert_pem, key_pem, dir: dir.into() });
        }
        std::fs::create_dir_all(dir)?;
        let mut p = CertificateParams::default();
        p.distinguished_name.push(DnType::CommonName, "EmuZip CA");
        p.distinguished_name.push(DnType::OrganizationName, "EmuZip");
        p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
        p.not_before = hudsucker::rcgen::date_time_ymd(2025, 1, 1);
        p.not_after = hudsucker::rcgen::date_time_ymd(2035, 12, 31);
        let key = KeyPair::generate()?;
        let cert = p.self_signed(&key)?;
        let (cert_pem, key_pem) = (cert.pem(), key.serialize_pem());
        std::fs::write(&c, &cert_pem)?;
        std::fs::write(&k, &key_pem)?;
        Ok(Self { cert_pem, key_pem, dir: dir.into() })
    }

    pub fn issuer(&self) -> Result<Issuer<'static, KeyPair>> {
        let key = KeyPair::from_pem(&self.key_pem)?;
        Issuer::from_ca_cert_pem(&self.cert_pem, key).context("CA inválido")
    }

    pub fn der(&self) -> Result<Vec<u8>> {
        pem_to_der(&self.cert_pem)
    }

    /// Nome usado no repositório de CAs do Android: `<subject_hash_old>.0`.
    pub fn android_name(&self) -> Result<String> {
        Ok(format!("{}.0", subject_hash_old(&self.der()?)?))
    }
}

pub fn pem_to_der(pem: &str) -> Result<Vec<u8>> {
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    Ok(base64::engine::general_purpose::STANDARD.decode(b64.trim())?)
}

/// Equivalente a `openssl x509 -subject_hash_old`: MD5 do subject (DER), 4 primeiros bytes little-endian.
pub fn subject_hash_old(der: &[u8]) -> Result<String> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).map_err(|e| anyhow::anyhow!("certificado inválido: {e}"))?;
    let d = Md5::digest(cert.tbs_certificate.subject.as_raw());
    Ok(format!("{:08x}", u32::from_le_bytes([d[0], d[1], d[2], d[3]])))
}

/// Script (executado como root) que injeta o CA no repositório do sistema, inclusive no APEX do Android 14+.
pub fn android_install_script(name: &str) -> String {
    format!(
        r#"set -e
T=/data/local/tmp/emuzip-ca
rm -rf $T; mkdir -p $T
if [ -d /apex/com.android.conscrypt/cacerts ]; then cp /apex/com.android.conscrypt/cacerts/* $T/; else cp /system/etc/security/cacerts/* $T/; fi
cp /data/local/tmp/{name} $T/
mount -t tmpfs tmpfs /system/etc/security/cacerts
cp $T/* /system/etc/security/cacerts/
chown root:root /system/etc/security/cacerts/*
chmod 644 /system/etc/security/cacerts/*
chcon u:object_r:system_file:s0 /system/etc/security/cacerts/* 2>/dev/null || true
if [ -d /apex/com.android.conscrypt/cacerts ]; then
  mount --bind /system/etc/security/cacerts /apex/com.android.conscrypt/cacerts
  for Z in $(pidof zygote zygote64 2>/dev/null); do
    nsenter --mount=/proc/$Z/ns/mnt -- mount --bind /system/etc/security/cacerts /apex/com.android.conscrypt/cacerts 2>/dev/null || true
    for P in $(ps -o PID -P $Z | grep -v PID); do
      nsenter --mount=/proc/$P/ns/mnt -- mount --bind /system/etc/security/cacerts /apex/com.android.conscrypt/cacerts 2>/dev/null || true
    done
  done
fi
echo EMUZIP_CA_OK"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_and_hash() {
        let dir = std::env::temp_dir().join(format!("emuzip-ca-test-{}", std::process::id()));
        let ca = Ca::load_or_create(&dir).unwrap();
        let name = ca.android_name().unwrap();
        assert_eq!(name.len(), 10);
        assert!(name.ends_with(".0"));
        // Recarregar mantém o mesmo CA.
        assert_eq!(Ca::load_or_create(&dir).unwrap().cert_pem, ca.cert_pem);
        assert!(ca.issuer().is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
