# Telemt Panel Export Schema

`GET /api/admin/export` returns a JSON snapshot with the following shape:

```json
{
  "version": 1,
  "exported_at": "2026-03-09T13:00:00Z",
  "settings": {
    "proxy_host": "proxy.example.com",
    "proxy_port": 443,
    "tls_domain": "example.com",
    "telemt_metrics_url": "http://localhost:9090/metrics",
    "telemt_ignore_time_skew": false
  },
  "users": [
    {
      "username": "alice",
      "secret": "0123456789abcdef0123456789abcdef",
      "enabled": true,
      "ip_limit": 3,
      "comment": "optional note",
      "status": "active",
      "data_limit": null,
      "max_connections": null,
      "expire_at": null
    }
  ]
}
```

## Required fields

- `version`: schema version. Current value is `1`.
- `exported_at`: RFC 3339 timestamp when the snapshot was created.
- `settings.proxy_host`: panel proxy host used for generated links.
- `settings.proxy_port`: panel proxy port.
- `settings.tls_domain`: fake TLS domain.
- `settings.telemt_ignore_time_skew`: exported runtime flag for `telemt` config generation.
- `users[].username`: unique user name.
- `users[].secret`: 32-byte hex string encoded as 32 lowercase/uppercase hex chars.
- `users[].enabled`: compatibility flag for older snapshots and tooling.

## Optional fields

- `settings.telemt_metrics_url`: metrics endpoint URL.
- `users[].ip_limit`: maps to panel field `max_unique_ips`.
- `users[].comment`: maps to panel field `note`.
- `users[].status`: explicit user status. If omitted on import, it is derived from `enabled`.
- `users[].data_limit`: user data quota in bytes.
- `users[].max_connections`: max TCP connections for the user.
- `users[].expire_at`: RFC 3339 expiration timestamp.

## Import behavior

- `POST /api/admin/import?mode=merge` updates existing users by `username` and creates missing users.
- `POST /api/admin/import?mode=replace` deletes all current users first, then recreates users from the snapshot.
- Snapshot `settings` are applied to the running panel process before `telemt` config regeneration.
- Imported `settings` are runtime-only today: they take effect immediately, but are not persisted to `.env` or another durable config store and will revert after a panel restart unless updated separately.
- Invalid user rows are skipped individually and returned in `skipped` with a reason.
- Unsupported snapshot versions return HTTP `400`.
