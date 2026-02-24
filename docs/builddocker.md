Примеры запуска:
Оба образа, текущая платформа, логин по переменным:
  DOCKERHUB_USERNAME=grandmax DOCKERHUB_TOKEN=ваш_токен ./scripts/docker-build-push.sh
Оба образа для amd64 и arm64 (через buildx):
  DOCKERHUB_USERNAME=grandmax DOCKERHUB_TOKEN=... ./scripts/docker-build-push.sh --multiarch
Только панель:
  ./scripts/docker-build-push.sh --panel-only
Только telemt:
  ./scripts/docker-build-push.sh --telemt-only

Локальная сборка и запуск с панелью (traefik + telemt + panel):
В каталоге temp/ подготовлены .env, docker-compose.yml и traefik/dynamic/tcp.yml. Запуск из корня репозитория:
  cd temp && docker-compose up --build -d
Панель: http://localhost:8080, прокси: порт 443. После изменений кода: docker-compose up --build -d.

Первый администратор панели (при ручной настройке) создаётся вручную:
  docker exec -it mtpanel-panel python -m app.cli create-admin --username admin --password "ВАШ_ПАРОЛЬ" --sudo
После этого входите в панель по логину admin и указанному паролю.

Параметр пользователя «Max unique IPs» (макс. уникальных IP): лимит одновременно подключённых уникальных IP-адресов для этого пользователя. Задаётся в панели, записывается в конфиг telemt в секцию [access.user_max_unique_ips]. При новом подключении с нового IP, если у пользователя уже подключено столько разных IP, сколько задано лимитом, соединение отклоняется; при отключении клиента соответствующий IP перестаёт учитываться.
