# Архитектура панели управления MTProxy (mtpannel)

Документ описывает взаимодействие панели управления с telemt MTProxy: контейнеры, API, схему БД, формат config.toml и протокол обмена данными.

## Интеграционная проверка (E2E)

Полный цикл проверки после установки в режиме «прокси + панель»:

1. **Установка:** `bash install.sh` → выбрать «Установить панель управления? (y/N)» → y. Указать каталог, порт, домен, собрать образы. Создать первого админа (логин/пароль).
2. **Запуск:** `cd ${INSTALL_DIR} && docker compose up -d`. Дождаться готовности: `docker compose exec panel curl -sf http://localhost:8080/health`.
3. **Проверка через скрипт:**  
   `BASE_URL=http://localhost:8080 ADMIN_USER=admin ADMIN_PASS=... ./temp/e2e_panel_verify.sh`  
   Скрипт выполняет: health → логин → создание пользователя → получение ссылок.
4. **Проверка config.toml (hot-reload):**  
   `docker compose exec panel cat /app/telemt-config/config.toml` — в секции `[access.users]` должен быть пользователь, созданный через панель. Telemt подхватывает изменения без перезапуска.
5. **Проверка ссылки:** подставить полученную ссылку в Telegram (Настройки → Данные и память → Использовать прокси).

## 1. Обзор контейнеров

| Контейнер           | Образ              | Порты (хост)   | Назначение                    |
|---------------------|--------------------|----------------|-------------------------------|
| mtpannel-traefik    | traefik:v3.6       | 443            | SNI-маршрутизация, TLS       |
| mtpannel-telemt     | telemt:local       | — (внутр. 1234)| MTProxy (Rust), метрики :9090 |
| mtpannel-panel      | panel:local        | 8080           | REST API + Web UI (FastAPI)  |

**Связи:**
- Panel и Telemt разделяют Docker volume с одним файлом `config.toml`. Panel пишет конфиг, Telemt читает и подхватывает изменения через hot-reload (inotify/poll).
- Panel по внутренней сети обращается к Telemt: `http://mtpannel-telemt:9090/metrics` для скрейпинга Prometheus.

## 2. API панели (REST, OpenAPI)

Базовый префикс: `/api`. Аутентификация: Bearer JWT (кроме `/api/admin/token` и `/health`).

### 2.1 Аутентификация

- **POST /api/admin/token**  
  Body: `{"username": "string", "password": "string"}`  
  Ответ 200: `{"access_token": "string", "token_type": "bearer"}`  
  Ответ 401: неверные учётные данные.

- Заголовок для защищённых эндпоинтов: `Authorization: Bearer <access_token>`.

### 2.2 Администраторы (sudo или сам себя)

- **GET /api/admins** — список админов (sudo).
- **POST /api/admins** — создать админа (sudo). Body: `{"username": "string", "password": "string", "is_sudo": false}`.
- **PUT /api/admins/{username}** — изменить пароль (sudo или свой логин).
- **DELETE /api/admins/{username}** — удалить админа (sudo, не себя).

### 2.3 Пользователи прокси

- **GET /api/users** — список пользователей.  
  Query: `offset`, `limit`, `search` (по username), `status` (active|disabled|limited|expired).  
  Ответ: `{"users": [...], "total": N}`.

- **POST /api/users** — создать пользователя.  
  Body: `{"username": "string", "data_limit": number|null, "max_connections": number|null, "max_unique_ips": number|null, "expire_at": "datetime|null", "note": "string"}`.  
  Секрет 32 hex генерируется автоматически.  
  Ответ 201: объект пользователя (включая `secret` и `proxy_links` один раз при создании).

- **GET /api/users/{username}** — один пользователь (включая ссылки и использование).

- **PUT /api/users/{username}** — обновить лимиты, статус, заметку, срок действия.  
  Body: частичное обновление полей.

- **DELETE /api/users/{username}** — удалить пользователя.

- **POST /api/users/{username}/regenerate-secret** — сгенерировать новый 32-hex секрет, перезаписать config.toml.  
  Ответ: объект пользователя с новыми `proxy_links`.

### 2.4 Ссылки и системная информация

- **GET /api/users/{username}/links** — ссылки для пользователя:  
  `{"tg_link": "tg://proxy?server=...&port=...&secret=ee...", "https_link": "https://t.me/proxy?..."}`.

- **GET /api/system/stats** — сводка для дашборда: uptime, total_connections, bad_connections, per-user traffic (из БД или последнего скрейпа).

- **GET /api/system/metrics** — проксирование или кэш метрик telemt (опционально).

### 2.5 Служебные

- **GET /health** — без авторизации. Ответ 200: `{"status": "ok"}`.

## 3. Схема БД (SQLite)

### 3.1 Таблица admins

| Колонка           | Тип        | Описание                    |
|-------------------|------------|-----------------------------|
| id                | INTEGER PK |                             |
| username          | TEXT UNIQUE| Логин                       |
| hashed_password   | TEXT       | bcrypt                      |
| is_sudo           | BOOLEAN    | Суперадмин                  |
| created_at        | DATETIME   | UTC                         |

### 3.2 Таблица users

| Колонка            | Тип        | Описание                          |
|--------------------|------------|-----------------------------------|
| id                 | INTEGER PK |                                    |
| username           | TEXT UNIQUE| Имя пользователя прокси (3–32 символа) |
| secret             | TEXT       | 32 hex (16 bytes)                  |
| status             | TEXT       | active, disabled, limited, expired |
| data_limit         | INTEGER NULL | Лимит трафика (байты), NULL = без лимита |
| data_used          | INTEGER    | Использовано (байты), по умолчанию 0 |
| max_connections    | INTEGER NULL | Лимит одновременных TCP-сессий   |
| max_unique_ips     | INTEGER NULL | Лимит уникальных IP              |
| expire_at          | DATETIME NULL | Окончание срока (UTC)            |
| note               | TEXT       | Заметка админа                    |
| created_at         | DATETIME   | UTC                               |
| created_by_admin_id| INTEGER FK | admins.id                         |

### 3.3 Таблица traffic_logs

| Колонка     | Тип        | Описание                |
|-------------|------------|-------------------------|
| id          | INTEGER PK |                         |
| user_id     | INTEGER FK | users.id                |
| octets_from | INTEGER    | байты от клиента        |
| octets_to   | INTEGER    | байты к клиенту         |
| recorded_at| DATETIME   | момент замера (UTC)      |

### 3.4 Таблица system_stats

| Колонка            | Тип        | Описание           |
|--------------------|------------|--------------------|
| id                 | INTEGER PK |                    |
| uptime             | REAL       | секунды            |
| total_connections  | INTEGER    |                    |
| bad_connections    | INTEGER    |                    |
| recorded_at        | DATETIME   | UTC                |

## 4. Формат config.toml и алгоритм мерджа

### 4.1 Секции, управляемые панелью (перезаписываются)

Панель формирует и перезаписывает только секцию `[access]` и подсекции:

- `[access]` — replay_check_len, replay_window_secs, ignore_time_skew (берутся из шаблона или дефолтов).
- `[access.users]` — таблица `username = "32_hex_secret"` для всех пользователей со статусом active и не expired/limited.
- `[access.user_max_tcp_conns]` — только для пользователей, у которых задан max_connections.
- `[access.user_data_quota]` — только для пользователей с data_limit.
- `[access.user_expirations]` — только для пользователей с expire_at; формат: `username = "YYYY-MM-DDTHH:MM:SSZ"` (UTC).
- `[access.user_max_unique_ips]` — только для пользователей с max_unique_ips.

Остальные секции telemt не трогаются.

### 4.2 Шаблон (не-пользовательские секции)

Источник шаблона при первом запуске панели с существующим config.toml:

- Читается текущий `config.toml`.
- Из него извлекаются и сохраняются в памяти/файле шаблона все секции кроме `[access.users]`, `[access.user_max_tcp_conns]`, `[access.user_data_quota]`, `[access.user_expirations]`, `[access.user_max_unique_ips]`. Секция `[access]` с replay_* и ignore_time_skew может браться из файла или дефолтов.

При отсутствии config.toml панель может создать минимальный конфиг из дефолтного шаблона (general, server, censorship, timeouts, upstreams), затем дописать access из БД.

### 4.3 Алгоритм записи config.toml

1. Загрузить шаблон (все секции кроме управляемых access-таблиц).
2. Из БД выбрать пользователей с status=active, expire_at > now (или NULL).
3. Построить словари: users, user_max_tcp_conns, user_data_quota, user_expirations, user_max_unique_ips.
4. Сформировать TOML: шаблон + `[access]` с подсекциями в формате telemt.
5. Атомарно записать в файл (например, во временный файл + rename), чтобы telemt не читал частично обновлённый конфиг.

## 5. Протокол обмена Panel ↔ Telemt

### 5.1 Конфиг (shared volume)

- Один и тот же файл `config.toml` монтируется в Telemt (например `/app/config.toml`) и в Panel (например `/app/telemt-config/config.toml`).
- Panel при каждой мутации пользователя (создание, обновление, удаление, regenerate-secret, смена статуса/лимитов) перегенерирует config.toml и записывает его на shared volume.
- Telemt уже поддерживает hot-reload по inotify/poll; перезапуск не требуется.

### 5.2 Метрики (HTTP)

- Panel по расписанию (например каждые 30 с) выполняет GET `http://mtpannel-telemt:9090/metrics`.
- Парсится текст Prometheus: строки вида  
  `telemt_user_octets_from_client{user="username"} N` и  
  `telemt_user_octets_to_client{user="username"} N`,  
  а также `telemt_uptime_seconds`, `telemt_connections_total`, `telemt_connections_bad_total`,  
  при необходимости `telemt_user_connections_current{user="..."}`.
- По дельтам с предыдущего скрейпа обновляются traffic_logs и users.data_used.
- Если у пользователя задан data_limit и data_used >= data_limit, статус переводится в limited, пользователь исключается из следующей генерации config.toml (и при следующей записи конфига будет удалён из access).

### 5.3 Формат ссылки FakeTLS (EE)

- Домен для маскировки задаётся в конфиге панели (TLS_DOMAIN / censorship.tls_domain).
- Ссылка:  
  `tg://proxy?server={host}&port={port}&secret=ee{secret_32hex}{domain_hex}`  
  где domain_hex = hex-кодирование UTF-8 строки домена (без префикса 0x).
- Альтернатива: `https://t.me/proxy?server=...&port=...&secret=...` с тем же secret.

## 6. Безопасность

- JWT: подпись и срок действия обязательны; SECRET_KEY только из окружения.
- Пароли админов: только bcrypt (или аналог), не хранить в открытом виде.
- Все запросы к БД через ORM/параметризованные запросы (без конкатенации SQL).
- Панель не экспортирует метрики telemt наружу; доступ к /metrics только из внутренней сети контейнеров.
- В production панель должна обслуживаться по HTTPS (обратный прокси перед контейнером).

## 7. Переменные окружения панели

| Переменная           | Описание                              | Пример |
|----------------------|---------------------------------------|--------|
| DATABASE_URL         | SQLite URL                            | sqlite:////app/data/panel.db |
| SECRET_KEY           | Секрет для JWT                        | строка из openssl rand |
| TELEMT_CONFIG_PATH   | Путь к config.toml на shared volume   | /app/telemt-config/config.toml |
| TELEMT_METRICS_URL   | URL метрик telemt                     | http://mtpannel-telemt:9090/metrics |
| PROXY_HOST           | Хост для ссылок (IP или домен)        | proxy.example.com |
| PROXY_PORT           | Порт для ссылок                       | 443 |
| TLS_DOMAIN           | Домен Fake TLS (EE)                   | example.com |

Эта спецификация используется при реализации бэкенда и фронтенда панели и при интеграции с install.sh и Docker.
