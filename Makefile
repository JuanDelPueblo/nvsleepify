PREFIX ?= /usr/local
BIN_DIR = $(PREFIX)/bin
BINARY_NAME = nvsleepify
DAEMON_BINARY_NAME = nvsleepifyd
TRAY_BINARY_NAME = nvsleepify-tray
TARGET_DIR = target/release
DBUS_CONF_DIR ?= /etc/dbus-1/system.d
APPLICATIONS_DIR = $(PREFIX)/share/applications
ICONS_DIR = $(PREFIX)/share/icons/hicolor/scalable/apps
BASH_COMPLETIONS_DIR = $(PREFIX)/share/bash-completion/completions
ZSH_COMPLETIONS_DIR = $(PREFIX)/share/zsh/site-functions
FISH_COMPLETIONS_DIR = $(PREFIX)/share/fish/vendor_completions.d

.PHONY: all build install uninstall clean

all: build

build:
	cargo build --release

install:
	install -d $(BIN_DIR)
	install -m 755 $(TARGET_DIR)/$(BINARY_NAME) $(BIN_DIR)/$(BINARY_NAME)
	install -m 755 $(TARGET_DIR)/$(DAEMON_BINARY_NAME) $(BIN_DIR)/$(DAEMON_BINARY_NAME)
	install -m 755 $(TARGET_DIR)/$(TRAY_BINARY_NAME) $(BIN_DIR)/$(TRAY_BINARY_NAME)
	install -d /etc/systemd/system
	install -m 644 nvsleepifyd.service /etc/systemd/system/nvsleepifyd.service
	install -d $(DBUS_CONF_DIR)
	install -m 644 org.nvsleepify.conf $(DBUS_CONF_DIR)/org.nvsleepify.conf
	install -d $(APPLICATIONS_DIR)
	install -m 644 nvsleepify-tray.desktop $(APPLICATIONS_DIR)/nvsleepify-tray.desktop
	install -d $(ICONS_DIR)
	install -m 644 icons/nvsleepify-gpu-active.svg $(ICONS_DIR)/nvsleepify-gpu-active.svg
	install -m 644 icons/nvsleepify-gpu-suspended.svg $(ICONS_DIR)/nvsleepify-gpu-suspended.svg
	install -m 644 icons/nvsleepify-gpu-off.svg $(ICONS_DIR)/nvsleepify-gpu-off.svg
	install -d $(BASH_COMPLETIONS_DIR)
	$(TARGET_DIR)/$(BINARY_NAME) completion bash > $(BASH_COMPLETIONS_DIR)/$(BINARY_NAME)
	chmod 644 $(BASH_COMPLETIONS_DIR)/$(BINARY_NAME)
	install -d $(ZSH_COMPLETIONS_DIR)
	$(TARGET_DIR)/$(BINARY_NAME) completion zsh > $(ZSH_COMPLETIONS_DIR)/_$(BINARY_NAME)
	chmod 644 $(ZSH_COMPLETIONS_DIR)/_$(BINARY_NAME)
	install -d $(FISH_COMPLETIONS_DIR)
	$(TARGET_DIR)/$(BINARY_NAME) completion fish > $(FISH_COMPLETIONS_DIR)/$(BINARY_NAME).fish
	chmod 644 $(FISH_COMPLETIONS_DIR)/$(BINARY_NAME).fish
	systemctl daemon-reload
	systemctl reload dbus || true
	@echo "Installed $(BINARY_NAME), $(DAEMON_BINARY_NAME), and $(TRAY_BINARY_NAME)"

uninstall:
	systemctl disable --now nvsleepifyd.service || true
	rm -f $(BASH_COMPLETIONS_DIR)/$(BINARY_NAME)
	rm -f $(ZSH_COMPLETIONS_DIR)/_$(BINARY_NAME)
	rm -f $(FISH_COMPLETIONS_DIR)/$(BINARY_NAME).fish
	rm -f $(BIN_DIR)/$(BINARY_NAME)
	rm -f $(BIN_DIR)/$(DAEMON_BINARY_NAME)
	rm -f $(BIN_DIR)/$(TRAY_BINARY_NAME)
	rm -f /etc/systemd/system/nvsleepifyd.service
	rm -f $(DBUS_CONF_DIR)/org.nvsleepify.conf
	rm -f $(APPLICATIONS_DIR)/nvsleepify-tray.desktop
	rm -f $(ICONS_DIR)/nvsleepify-gpu-active.svg
	rm -f $(ICONS_DIR)/nvsleepify-gpu-suspended.svg
	rm -f $(ICONS_DIR)/nvsleepify-gpu-off.svg
	systemctl daemon-reload
	systemctl reload dbus || true
	@echo "Uninstalled $(BINARY_NAME)"

clean:
	cargo clean
