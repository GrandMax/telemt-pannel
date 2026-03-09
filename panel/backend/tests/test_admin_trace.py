from fastapi import HTTPException

from app.models.admin import Admin
from app.utils.auth import create_access_token
from app.utils.auth import hash_password


def _auth_headers(admin_username: str = "sudo", is_sudo: bool = True) -> dict[str, str]:
    token = create_access_token(admin_username, {"is_sudo": is_sudo})
    return {"Authorization": f"Bearer {token}"}


def test_trace_sessions_endpoint_requires_sudo(client, db, sudo_admin):
    db.add(
        Admin(
            username="ops",
            hashed_password=hash_password("ops-pass"),
            is_sudo=False,
        )
    )
    db.commit()

    print("step: call trace sessions as non-sudo admin")
    response = client.get(
        "/api/admin/trace/sessions",
        headers=_auth_headers(admin_username="ops", is_sudo=False),
    )

    print("step: verify access is denied")
    assert response.status_code == 403, response.text


def test_trace_sessions_endpoint_filters_user_and_dc(client, sudo_admin, monkeypatch):
    print("step: stub telemt trace sessions payload")

    async def fake_fetch(path: str):
        assert path == "/trace/sessions?limit=50"
        return {
            "sessions": [
                {"conn_id": 10, "user": "alice", "target_dc": 2, "state": "active"},
                {"conn_id": 11, "user": "bob", "target_dc": 4, "state": "recent"},
                {"conn_id": 12, "user": "alice", "target_dc": 4, "state": "active"},
            ]
        }

    monkeypatch.setattr("app.routers.admin_trace.fetch_telemt_trace_json", fake_fetch)

    print("step: request filtered sessions list")
    response = client.get(
        "/api/admin/trace/sessions?user=alice&dc=4",
        headers=_auth_headers(),
    )

    print("step: verify backend keeps only matching sessions")
    assert response.status_code == 200, response.text
    payload = response.json()
    assert len(payload["sessions"]) == 1
    assert payload["sessions"][0]["conn_id"] == 12


def test_trace_detail_endpoint_passes_limit_and_404(client, sudo_admin, monkeypatch):
    print("step: stub successful trace detail request")

    async def fake_fetch(path: str):
        if path == "/trace/77?limit=2":
            return {
                "conn_id": 77,
                "user": "alice",
                "target_dc": 2,
                "state": "active",
                "events": [
                    {"seq": 2, "timestamp_ms": 2, "kind": "frame", "message": "payload"},
                    {"seq": 3, "timestamp_ms": 3, "kind": "ack", "message": "quickack"},
                ],
            }
        raise HTTPException(status_code=404, detail="Trace session not found")

    monkeypatch.setattr("app.routers.admin_trace.fetch_telemt_trace_json", fake_fetch)

    print("step: request trace detail with limit")
    response = client.get(
        "/api/admin/trace/77?limit=2",
        headers=_auth_headers(),
    )

    print("step: verify limited detail payload is returned")
    assert response.status_code == 200, response.text
    payload = response.json()
    assert payload["conn_id"] == 77
    assert len(payload["events"]) == 2

    print("step: request missing trace detail and preserve 404")
    missing = client.get(
        "/api/admin/trace/999?limit=2",
        headers=_auth_headers(),
    )
    assert missing.status_code == 404, missing.text
