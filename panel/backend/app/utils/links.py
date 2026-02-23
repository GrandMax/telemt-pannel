"""Generate Telegram MTProxy links (FakeTLS / EE format)."""
from app.config import settings


def domain_hex(domain: str) -> str:
    """UTF-8 domain as hex string (no 0x prefix)."""
    return domain.encode("utf-8").hex()


def proxy_links(secret_32hex: str) -> dict[str, str]:
    """Build tg:// and https://t.me/proxy links for EE (FakeTLS) format."""
    host = settings.proxy_host
    port = settings.proxy_port
    domain = settings.tls_domain
    secret_part = f"ee{secret_32hex}{domain_hex(domain)}"
    tg = f"tg://proxy?server={host}&port={port}&secret={secret_part}"
    https = f"https://t.me/proxy?server={host}&port={port}&secret={secret_part}"
    return {"tg_link": tg, "https_link": https}
