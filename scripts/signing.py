#!/usr/bin/env python3
"""Local signing identity and guarded installation. Never exports an existing key."""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import tempfile
import time

NAME = "RSX Local Code Signing"
BUNDLE_ID = "app.rxs.capture"
CONFIG = Path(os.environ.get("RSX_SIGNING_CONFIG", str(Path.home() / "Library/Application Support/RSX/signing.json")))


def run(*args, **kwargs):
    try:
        return subprocess.run(args, check=True, text=True, stdout=subprocess.PIPE,
                              stderr=subprocess.PIPE, **kwargs).stdout.strip()
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f"{Path(args[0]).name} {args[1]}: {error.stderr.strip()[:800]}") from error


def load_identity():
    try:
        config = json.loads(CONFIG.read_text())
    except (OSError, ValueError) as error:
        raise RuntimeError("Identidade local ausente. Execute python3 scripts/signing.py setup uma vez.") from error
    fingerprint = config.get("fingerprint", "")
    keychain = config.get("keychain", "")
    if not re.fullmatch(r"[A-Fa-f0-9]{40}", fingerprint) or not isinstance(keychain, str) or not keychain:
        raise RuntimeError("Configuração de assinatura inválida; restaure a identidade existente.")
    identities = run("/usr/bin/security", "find-identity", "-v", "-p", "codesigning", keychain)
    if fingerprint.upper() not in identities.upper():
        raise RuntimeError("Certificado inacessível ou inválido. Desbloqueie o Chaves ou restaure a identidade; nenhum certificado novo será criado.")
    return fingerprint.upper(), keychain


def setup():
    if CONFIG.exists():
        load_identity()
        print("Identidade existente preservada.")
        return
    keychain = run("/usr/bin/security", "default-keychain", "-d", "user").strip('"')
    existing = subprocess.run(["/usr/bin/security", "find-certificate", "-c", NAME, keychain],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if existing.returncode == 0:
        raise RuntimeError("Já existe um certificado RSX. Restaure signing.json; não será criada uma identidade duplicada.")
    if existing.returncode != 44:
        raise RuntimeError("Não foi possível consultar o Chaves. Nenhuma identidade nova será criada.")
    CONFIG.parent.mkdir(parents=True, exist_ok=True)
    # Unencrypted import material only exists inside this private temporary directory.
    with tempfile.TemporaryDirectory(prefix="rsx-signing-") as temporary:
        work = Path(temporary)
        os.chmod(work, 0o700)
        extension = work / "certificate.cnf"
        extension.write_text("""[req]
prompt = no
distinguished_name = subject
x509_extensions = codesign
[subject]
CN = RSX Local Code Signing
O = RSX Local
[codesign]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
""")
        key, certificate, archive = (work / name for name in ("key.pem", "certificate.pem", "identity.p12"))
        run("/usr/bin/openssl", "req", "-new", "-newkey", "rsa:3072", "-nodes", "-x509",
            "-sha256", "-days", "10950", "-config", str(extension), "-keyout", str(key), "-out", str(certificate))
        os.chmod(key, 0o600)
        fingerprint = run("/usr/bin/openssl", "x509", "-in", str(certificate), "-noout", "-fingerprint", "-sha1").split("=", 1)[1].replace(":", "")
        password = secrets.token_hex(32)
        run("/usr/bin/openssl", "pkcs12", "-export", "-inkey", str(key), "-in", str(certificate),
            "-out", str(archive), "-passout", "stdin", input=password + "\n")
        os.chmod(archive, 0o600)
        run("/usr/bin/security", "import", str(archive), "-k", keychain, "-f", "pkcs12", "-P", password, "-x", "-T", "/usr/bin/codesign")
        # User-domain trust applies only to code signing, never TLS or other uses.
        run("/usr/bin/security", "add-trusted-cert", "-r", "trustRoot", "-p", "codeSign", "-k", keychain, str(certificate))
        config = {"name": NAME, "fingerprint": fingerprint, "keychain": keychain}
        with CONFIG.open("x") as output:
            os.chmod(CONFIG, 0o600)
            json.dump(config, output, indent=2)
            output.write("\n")
    load_identity()
    print("Identidade local criada no Chaves. Configuração:", CONFIG)


def info(app):
    result = subprocess.run(["/usr/bin/codesign", "-d", "-r-", str(app)], check=True,
                            text=True, capture_output=True)
    return result.stdout + result.stderr


def requirement(app):
    match = re.search(r"^#? ?designated => (.+)$", info(app), re.MULTILINE)
    if not match or "cdhash" in match[1]:
        raise RuntimeError("O aplicativo não possui uma identidade estável.")
    return match[1]


def bundle_identifier(app):
    return run("/usr/libexec/PlistBuddy", "-c", "Print :CFBundleIdentifier", str(app / "Contents/Info.plist"))


def validate(app):
    if bundle_identifier(app) != BUNDLE_ID:
        raise RuntimeError("Identificador do app incompatível.")
    run("/usr/bin/codesign", "--verify", "--strict", str(app))
    return requirement(app)


def sign(app):
    override = os.environ.get("RSX_CODESIGN_IDENTITY") or os.environ.get("RXS_CODESIGN_IDENTITY")
    if override:
        if override == "-":
            raise RuntimeError("Para diagnósticos ad-hoc use RSX_ALLOW_ADHOC=1 explicitamente.")
        run("/usr/bin/codesign", "--force", "--options", "runtime", "--timestamp", "--sign", override, str(app))
    elif os.environ.get("RSX_ALLOW_ADHOC") == "1":
        run("/usr/bin/codesign", "--force", "--sign", "-", str(app))
        print("Aviso: pacote ad-hoc de diagnóstico/CI; não instalar sobre o app de uso diário.")
        return
    else:
        fingerprint, keychain = load_identity()
        # Pin the certificate and identifier, not the changing executable hash.
        designated = f'identifier "{BUNDLE_ID}" and certificate leaf = H"{fingerprint}"'
        run("/usr/bin/codesign", "--force", "--timestamp=none", "--keychain", keychain,
            "--sign", fingerprint, "-r", "=designated => " + designated, str(app))
    validate(app)
    print("Assinatura estável verificada.")


def preflight():
    if os.environ.get("RSX_ALLOW_ADHOC") == "1":
        return
    override = os.environ.get("RSX_CODESIGN_IDENTITY") or os.environ.get("RXS_CODESIGN_IDENTITY")
    if override:
        identities = run("/usr/bin/security", "find-identity", "-v", "-p", "codesigning")
        if override == "-" or override not in identities:
            raise RuntimeError("Identidade de assinatura configurada indisponível.")
    else:
        load_identity()


def installed_pids(target):
    executable = str(target / "Contents/MacOS/rsx")
    rows = run("/bin/ps", "-axo", "pid=,comm=").splitlines()
    return [int(parts[0]) for row in rows if len(parts := row.strip().split(None, 1)) == 2 and parts[1] == executable]


def install(source, target, migrate):
    validate(source)
    # A stable local candidate must also belong to the configured signer. This
    # prevents a first migration accepting an unrelated self-signed certificate.
    if not (os.environ.get("RSX_CODESIGN_IDENTITY") or os.environ.get("RXS_CODESIGN_IDENTITY")):
        fingerprint, _ = load_identity()
        expected = f'identifier "{BUNDLE_ID}" and certificate leaf = H"{fingerprint}"'
        run("/usr/bin/codesign", "--verify", "--strict", "-R", "=" + expected, str(source))
    if target.exists():
        if bundle_identifier(target) != BUNDLE_ID:
            raise RuntimeError("O destino contém outro aplicativo.")
        run("/usr/bin/codesign", "--verify", "--strict", str(target))
        current = info(target)
        if "cdhash" in current:
            if not migrate:
                raise RuntimeError("Migração inicial exige --migrate-adhoc. Nenhum app foi alterado.")
        else:
            # New candidate must satisfy the identity already authorized by macOS.
            run("/usr/bin/codesign", "--verify", "--strict", "-R", "=" + requirement(target), str(source))
    target.parent.mkdir(parents=True, exist_ok=True)
    # Stage on the destination volume, so replacement and rollback use rename.
    with tempfile.TemporaryDirectory(prefix=".RSX-install-", dir=target.parent) as temporary:
        candidate = Path(temporary) / "RSX.app"
        run("/usr/bin/ditto", str(source), str(candidate))
        validate(candidate)
        if installed_pids(target):
            run("/usr/bin/osascript", "-e", "tell application " + json.dumps(str(target)) + " to quit", timeout=45)
            deadline = time.monotonic() + 30
            while installed_pids(target):
                if time.monotonic() >= deadline:
                    raise RuntimeError("O RSX permaneceu aberto. Cancele ou exporte suas alterações; a instalação atual foi preservada.")
                time.sleep(0.25)
        backup = target.parent / (".RSX-backup-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S-%f") + ".app")
        old_moved = False
        new_moved = False
        try:
            if target.exists():
                target.rename(backup)
                old_moved = True
            candidate.rename(target)
            new_moved = True
            validate(target)
        except Exception:
            if new_moved:
                shutil.rmtree(target)
            if old_moved:
                backup.rename(target)
            raise
    print("Instalado:", target)
    if old_moved:
        print("Cópia de recuperação:", backup)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("setup")
    commands.add_parser("preflight")
    for name in ("sign", "check"):
        command = commands.add_parser(name)
        command.add_argument("app", type=Path)
    command = commands.add_parser("install")
    command.add_argument("--source", type=Path, default=Path("dist/RSX.app"))
    command.add_argument("--target", type=Path, default=Path("/Applications/RSX.app"))
    command.add_argument("--migrate-adhoc", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "setup":
            setup()
        elif args.command == "preflight":
            preflight()
        elif args.command == "sign":
            sign(args.app)
        elif args.command == "check":
            print(validate(args.app))
        else:
            install(args.source.resolve(), args.target.resolve(), args.migrate_adhoc)
    except (RuntimeError, OSError, subprocess.SubprocessError) as error:
        # Do not dump commands or subprocess output from key import operations.
        parser.exit(1, f"Falha: {error if isinstance(error, (RuntimeError, OSError)) else 'operação nativa recusada ou indisponível'}\n")


if __name__ == "__main__":
    main()
