"""Create a self-signed macOS code-signing certificate for personal builds.

A stable signing identity keeps macOS privacy grants (Input Monitoring) across app
updates; ad-hoc signatures change on every build, so the grant would be lost.

Writes <out_dir>/codesign.p12 (private key, keep out of git), codesign.cer (public)
and codesign-password.txt.

Usage: python -I make_signing_cert.py <out_dir>
"""
import datetime
import os
import secrets
import sys

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import pkcs12
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

COMMON_NAME = "Wuwa Tappo Personal Signing"

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, COMMON_NAME)])
now = datetime.datetime.now(datetime.timezone.utc)
cert = (
    x509.CertificateBuilder()
    .subject_name(name)
    .issuer_name(name)
    .public_key(key.public_key())
    .serial_number(x509.random_serial_number())
    .not_valid_before(now - datetime.timedelta(days=1))
    .not_valid_after(now + datetime.timedelta(days=365 * 20))
    .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
    .add_extension(
        x509.KeyUsage(
            digital_signature=True, content_commitment=False, key_encipherment=False,
            data_encipherment=False, key_agreement=False, key_cert_sign=False,
            crl_sign=False, encipher_only=False, decipher_only=False,
        ),
        critical=True,
    )
    .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.CODE_SIGNING]), critical=True)
    .sign(key, hashes.SHA256())
)
password = secrets.token_urlsafe(24)
# macOS `security import` cannot read the default modern PKCS#12 encryption.
encryption = (
    serialization.PrivateFormat.PKCS12.encryption_builder()
    .kdf_rounds(2048)
    .key_cert_algorithm(pkcs12.PBES.PBESv1SHA1And3KeyTripleDESCBC)
    .hmac_hash(hashes.SHA1())
    .build(password.encode())
)
with open(os.path.join(out, "codesign.p12"), "wb") as f:
    f.write(pkcs12.serialize_key_and_certificates(b"wuwatappo", key, cert, None, encryption))
with open(os.path.join(out, "codesign.cer"), "wb") as f:
    f.write(cert.public_bytes(serialization.Encoding.PEM))
with open(os.path.join(out, "codesign-password.txt"), "w") as f:
    f.write(password)
print("identity:", COMMON_NAME)
print("written to", out)
