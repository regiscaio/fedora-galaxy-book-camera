#!/usr/bin/env bash

set -euo pipefail

readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly APP_ID="com.caioregis.GalaxyBookCamera"
readonly MAKE_CMD="${MAKE:-make}"
readonly APP_BINARY="${REPO_ROOT}/target/release/galaxybook-camera"
readonly ICON_SOURCE="${REPO_ROOT}/assets/galaxybook-camera.svg"
readonly COUNTDOWN_ICON_SOURCE="${REPO_ROOT}/assets/camera-timer-symbolic.svg"
readonly TUNING_SOURCE="${REPO_ROOT}/data/libcamera/simple/ov02c10.yaml"
readonly DESKTOP_SOURCE="${REPO_ROOT}/data/${APP_ID}.desktop"
readonly LOCAL_BIN_DIR="${HOME}/.local/bin"
readonly LOCAL_APPS_DIR="${HOME}/.local/share/applications"
readonly LOCAL_ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"
readonly LOCAL_ACTION_ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/actions"
readonly LOCAL_TUNING_DIR="${HOME}/.local/share/galaxybook-camera/libcamera/simple"
readonly WRAPPER_PATH="${LOCAL_BIN_DIR}/galaxybook-camera"
readonly DESKTOP_ENTRY_PATH="${LOCAL_APPS_DIR}/${APP_ID}.desktop"
readonly ICON_TARGET_PATH="${LOCAL_ICON_DIR}/${APP_ID}.svg"
readonly COUNTDOWN_ICON_TARGET_PATH="${LOCAL_ACTION_ICON_DIR}/camera-timer-symbolic.svg"
readonly TUNING_TARGET_PATH="${LOCAL_TUNING_DIR}/ov02c10.yaml"

require_cmd() {
	command -v "$1" >/dev/null 2>&1 || {
		echo "Missing required command: $1" >&2
		exit 1
	}
}

desktop_dir() {
	if command -v xdg-user-dir >/dev/null 2>&1; then
		xdg-user-dir DESKTOP
	else
		printf '%s\n' "${HOME}/Desktop"
	fi
}

main() {
	require_cmd bash
	require_cmd "${MAKE_CMD}"
	"${MAKE_CMD}" -C "${REPO_ROOT}" build >/dev/null

	if [[ ! -x "${APP_BINARY}" ]]; then
		echo "Built camera binary not found: ${APP_BINARY}" >&2
		exit 1
	fi

	mkdir -p "${LOCAL_BIN_DIR}" "${LOCAL_APPS_DIR}"
	mkdir -p "${LOCAL_ICON_DIR}" "${LOCAL_ACTION_ICON_DIR}" "${LOCAL_TUNING_DIR}"

	if [[ ! -f "${ICON_SOURCE}" ]]; then
		echo "Camera icon not found: ${ICON_SOURCE}" >&2
		exit 1
	fi
	if [[ ! -f "${COUNTDOWN_ICON_SOURCE}" ]]; then
		echo "Countdown icon not found: ${COUNTDOWN_ICON_SOURCE}" >&2
		exit 1
	fi
	if [[ ! -f "${TUNING_SOURCE}" ]]; then
		echo "OV02C10 tuning file not found: ${TUNING_SOURCE}" >&2
		exit 1
	fi
	if [[ ! -f "${DESKTOP_SOURCE}" ]]; then
		echo "Desktop entry source not found: ${DESKTOP_SOURCE}" >&2
		exit 1
	fi

	install -m 0644 "${ICON_SOURCE}" "${ICON_TARGET_PATH}"
	install -m 0644 "${COUNTDOWN_ICON_SOURCE}" "${COUNTDOWN_ICON_TARGET_PATH}"
	install -m 0644 "${TUNING_SOURCE}" "${TUNING_TARGET_PATH}"

cat >"${WRAPPER_PATH}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
LOG_DIR="\${XDG_CACHE_HOME:-\${HOME}/.cache}/galaxybook-camera"
LOG_FILE="\${LOG_DIR}/launcher.log"
mkdir -p "\${LOG_DIR}"
{
	printf '=== %s ===\\n' "\$(date --iso-8601=seconds)"
	printf 'argv:'
	printf ' %q' "\$@"
	printf '\\n'
} >>"\${LOG_FILE}"
if [[ "\${XDG_SESSION_TYPE:-}" == "wayland" && -n "\${WAYLAND_DISPLAY:-}" ]]; then
	exec env -u DISPLAY RUST_BACKTRACE=1 "${APP_BINARY}" "\$@" >>"\${LOG_FILE}" 2>&1
else
	exec env RUST_BACKTRACE=1 "${APP_BINARY}" "\$@" >>"\${LOG_FILE}" 2>&1
fi
EOF
	chmod 0755 "${WRAPPER_PATH}"
	sed \
		-e "s|@EXEC@|${WRAPPER_PATH}|g" \
		-e "s|@ICON@|${APP_ID}|g" \
		-e "s|@STARTUP_WM_CLASS@|${APP_ID}|g" \
		"${DESKTOP_SOURCE}" >"${DESKTOP_ENTRY_PATH}"

	if command -v desktop-file-validate >/dev/null 2>&1; then
		desktop-file-validate "${DESKTOP_ENTRY_PATH}"
	fi

	if command -v update-desktop-database >/dev/null 2>&1; then
		update-desktop-database "${LOCAL_APPS_DIR}" || true
	fi

	if command -v gtk-update-icon-cache >/dev/null 2>&1; then
		gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true
	fi

	local desktop_target_dir desktop_shortcut
	desktop_target_dir="$(desktop_dir)"
	desktop_shortcut="${desktop_target_dir}/Galaxy Book Camera.desktop"
	if [[ -d "${desktop_target_dir}" ]]; then
		install -m 0755 "${DESKTOP_ENTRY_PATH}" "${desktop_shortcut}"
		if command -v gio >/dev/null 2>&1; then
			gio set "${desktop_shortcut}" metadata::trusted true || true
		fi
	fi

	echo "Installed launcher:"
	echo "  ${DESKTOP_ENTRY_PATH}"
	echo "Wrapper:"
	echo "  ${WRAPPER_PATH}"
	echo "Icon:"
	echo "  ${ICON_TARGET_PATH}"
	echo "Action icon:"
	echo "  ${COUNTDOWN_ICON_TARGET_PATH}"
	echo "OV02C10 tuning:"
	echo "  ${TUNING_TARGET_PATH}"
	if [[ -d "$(desktop_dir)" ]]; then
		echo "Desktop shortcut:"
		echo "  ${desktop_shortcut}"
	fi
}

main "$@"
