from datetime import datetime, timezone

from app.models.user import User
from app.utils.auth import create_access_token


def _auth_headers(admin_username: str = "sudo", is_sudo: bool = True) -> dict[str, str]:
    token = create_access_token(admin_username, {"is_sudo": is_sudo})
    return {"Authorization": f"Bearer {token}"}


def _seed_user(
    db,
    *,
    username: str,
    secret: str,
    status: str = "active",
    max_unique_ips: int | None = None,
    note: str | None = None,
) -> User:
    user = User(
        username=username,
        secret=secret,
        status=status,
        data_used=0,
        max_unique_ips=max_unique_ips,
        note=note,
    )
    db.add(user)
    db.commit()
    db.refresh(user)
    return user


def test_export_snapshot_returns_users_and_settings(client, db, sudo_admin):
    print("step: seed two users for export snapshot")
    _seed_user(db, username="alice", secret="a" * 32, max_unique_ips=2, note="first user")
    _seed_user(db, username="bob", secret="b" * 32, status="disabled", note="disabled user")

    print("step: call GET /api/admin/export")
    response = client.get("/api/admin/export", headers=_auth_headers())

    print("step: verify export shape and mapped fields")
    assert response.status_code == 200, response.text
    payload = response.json()
    assert payload["version"] == 1
    assert "exported_at" in payload
    assert payload["settings"]["tls_domain"] == "example.com"

    users = {user["username"]: user for user in payload["users"]}
    assert users["alice"]["secret"] == "a" * 32
    assert users["alice"]["ip_limit"] == 2
    assert users["alice"]["enabled"] is True
    assert users["alice"]["comment"] == "first user"
    assert users["bob"]["enabled"] is False


def test_import_merge_adds_and_updates_users(client, db, sudo_admin):
    print("step: seed an existing user for merge import")
    _seed_user(db, username="alice", secret="a" * 32, max_unique_ips=1, note="old note")

    print("step: submit merge import payload with update and insert")
    response = client.post(
        "/api/admin/import?mode=merge",
        headers=_auth_headers(),
        json={
            "version": 1,
            "exported_at": datetime.now(timezone.utc).isoformat(),
            "settings": {
                "proxy_host": "proxy.example.com",
                "proxy_port": 443,
                "tls_domain": "example.com",
                "telemt_metrics_url": "http://localhost:9090/metrics",
                "telemt_ignore_time_skew": False,
            },
            "users": [
                {
                    "username": "alice",
                    "secret": "c" * 32,
                    "enabled": True,
                    "ip_limit": 3,
                    "comment": "updated note",
                },
                {
                    "username": "charlie",
                    "secret": "d" * 32,
                    "enabled": False,
                    "ip_limit": 5,
                    "comment": "new user",
                },
            ],
        },
    )

    print("step: verify import counters and persisted user data")
    assert response.status_code == 200, response.text
    payload = response.json()
    assert payload["added"] == 1
    assert payload["updated"] == 1
    assert payload["skipped"] == []

    users = {user.username: user for user in db.query(User).all()}
    assert users["alice"].secret == "c" * 32
    assert users["alice"].max_unique_ips == 3
    assert users["alice"].note == "updated note"
    assert users["charlie"].status == "disabled"


def test_import_replace_removes_old_users(client, db, sudo_admin):
    print("step: seed old users that should be replaced")
    _seed_user(db, username="legacy", secret="e" * 32)
    _seed_user(db, username="obsolete", secret="f" * 32, status="disabled")

    print("step: submit replace import payload")
    response = client.post(
        "/api/admin/import?mode=replace",
        headers=_auth_headers(),
        json={
            "version": 1,
            "exported_at": datetime.now(timezone.utc).isoformat(),
            "settings": {
                "proxy_host": "proxy.example.com",
                "proxy_port": 443,
                "tls_domain": "example.com",
                "telemt_metrics_url": "http://localhost:9090/metrics",
                "telemt_ignore_time_skew": False,
            },
            "users": [
                {
                    "username": "fresh",
                    "secret": "1" * 32,
                    "enabled": True,
                    "ip_limit": 7,
                    "comment": "fresh import",
                }
            ],
        },
    )

    print("step: verify old rows are gone and new snapshot is applied")
    assert response.status_code == 200, response.text
    payload = response.json()
    assert payload["added"] == 1
    assert payload["updated"] == 0
    usernames = [user.username for user in db.query(User).order_by(User.username).all()]
    assert usernames == ["fresh"]


def test_import_validation_skips_bad_records_and_rejects_bad_version(client, db, sudo_admin):
    print("step: reject unsupported snapshot version")
    bad_version = client.post(
        "/api/admin/import?mode=merge",
        headers=_auth_headers(),
        json={
            "version": 2,
            "exported_at": datetime.now(timezone.utc).isoformat(),
            "settings": {
                "proxy_host": "proxy.example.com",
                "proxy_port": 443,
                "tls_domain": "example.com",
                "telemt_metrics_url": "http://localhost:9090/metrics",
                "telemt_ignore_time_skew": False,
            },
            "users": [],
        },
    )
    assert bad_version.status_code == 400, bad_version.text

    print("step: skip malformed user rows without aborting whole import")
    response = client.post(
        "/api/admin/import?mode=merge",
        headers=_auth_headers(),
        json={
            "version": 1,
            "exported_at": datetime.now(timezone.utc).isoformat(),
            "settings": {
                "proxy_host": "proxy.example.com",
                "proxy_port": 443,
                "tls_domain": "example.com",
                "telemt_metrics_url": "http://localhost:9090/metrics",
                "telemt_ignore_time_skew": False,
            },
            "users": [
                {
                    "username": "",
                    "secret": "short",
                    "enabled": True,
                },
                {
                    "username": "good",
                    "secret": "2" * 32,
                    "enabled": True,
                    "comment": "valid user",
                },
            ],
        },
    )

    print("step: verify one valid user imported and one skipped with reason")
    assert response.status_code == 200, response.text
    payload = response.json()
    assert payload["added"] == 1
    assert payload["updated"] == 0
    assert len(payload["skipped"]) == 1
    assert payload["skipped"][0]["reason"]
    assert db.query(User).filter(User.username == "good").count() == 1
