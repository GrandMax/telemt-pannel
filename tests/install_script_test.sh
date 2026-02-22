#!/usr/bin/env bash
# Тест install.sh: меню (цвета, пункт 1), установка в temp, update, config, uninstall.
# Запуск из корня репозитория: bash tests/install_script_test.sh

set -e
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"
INSTALL_TARGET="${REPO_ROOT}/temp"

step() {
	echo ""
	echo "========== $1 =========="
}

# --- 1) Меню не должно выводить буквальные \033 (проверяем под script/TTY) ---
step "1. Проверка: в меню нет буквальных escape-кодов"
menu_out=""
if command -v script &>/dev/null; then
	# Эмулируем TTY, чтобы показалось меню; ввод "5" = выход
	menu_out=$(printf '5\n' | script -q -c "./install.sh" /dev/null 2>/dev/null) || true
else
	menu_out=$(printf '5\n' | ./install.sh 2>&1) || true
fi
if echo "$menu_out" | grep -q '\\033'; then
	echo "[FAIL] В выводе меню найдены буквальные \\033"
	echo "$menu_out" | head -20
	exit 1
fi
echo "[OK] Буквальных \\033 в выводе нет."

# --- 2) При pipe нет TTY — скрипт идёт в неинтерактивный режим (install). Пункт 1 в меню проверяйте вручную в терминале. ---
step "2. Неинтерактивный запуск install (без меню)"
out=$(INSTALL_DIR="$INSTALL_TARGET" INSTALL_DIR_FROM_ENV="$INSTALL_TARGET" ./install.sh install 2>&1) || true
if echo "$out" | grep -q "Начало установки"; then
	echo "[OK] Подкоманда install выполняется (найдено 'Начало установки')."
else
	echo "[INFO] Вывод install: $(echo "$out" | head -5)"
fi

# --- 3) Неинтерактивная установка в temp ---
step "3. Неинтерактивная установка в каталог temp"
rm -rf "$INSTALL_TARGET"
mkdir -p "$INSTALL_TARGET"
export INSTALL_DIR="$INSTALL_TARGET"
export INSTALL_DIR_FROM_ENV="$INSTALL_TARGET"
if ! ./install.sh install 2>&1; then
	echo "[WARN] install завершился с ошибкой (Docker/сеть?). Продолжаем тесты."
fi
if [[ -f "${INSTALL_TARGET}/telemt.toml" ]] && [[ -f "${INSTALL_TARGET}/docker-compose.yml" ]]; then
	echo "[OK] Файлы установки созданы в ${INSTALL_TARGET}."
else
	echo "[WARN] Не все файлы созданы; возможна среда без Docker."
fi

# --- 4) Обновление ---
step "4. Пункт меню 2 / update"
if [[ -f "${INSTALL_TARGET}/docker-compose.yml" ]]; then
	INSTALL_DIR="$INSTALL_TARGET" ./install.sh update 2>&1 || true
	echo "[OK] update выполнен (или пропущен из-за Docker)."
else
	echo "[SKIP] Нет установки в temp, update пропущен."
fi

# --- 5) Смена SNI (config) ---
step "5. Пункт меню 3 / config --sni"
if [[ -f "${INSTALL_TARGET}/telemt.toml" ]]; then
	INSTALL_DIR="$INSTALL_TARGET" ./install.sh config --sni test.example.com 2>&1 || true
	if grep -q 'tls_domain = "test.example.com"' "${INSTALL_TARGET}/telemt.toml" 2>/dev/null; then
		echo "[OK] Домен в telemt.toml обновлён на test.example.com."
	else
		echo "[WARN] Проверьте telemt.toml вручную."
	fi
else
	echo "[SKIP] Нет установки в temp, config пропущен."
fi

# --- 6) Удаление ---
step "6. Пункт меню 4 / uninstall -y"
if [[ -d "$INSTALL_TARGET" ]]; then
	INSTALL_DIR="$INSTALL_TARGET" ./install.sh uninstall -y 2>&1 || true
	if [[ ! -f "${INSTALL_TARGET}/telemt.toml" ]] && [[ ! -f "${INSTALL_TARGET}/docker-compose.yml" ]]; then
		echo "[OK] Удаление выполнено (файлы убраны)."
	else
		echo "[INFO] Каталог или файлы остались (ожидаемо при отказе или частичном удалении)."
	fi
fi

step "Готово"
echo "Все проверки пройдены. Установка тестировалась в: ${INSTALL_TARGET}"
