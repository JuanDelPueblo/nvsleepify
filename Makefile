PREFIX ?= /usr/local
BIN_DIR = $(PREFIX)/bin
BINARY_NAME = nvsleepify
DAEMON_BINARY_NAME = nvsleepifyd
TARGET_DIR = target/release
DBUS_CONF_DIR ?= /etc/dbus-1/system.d

.PHONY: all build install uninstall clean

all: build

build:
	cargo build --release

install: build
	install -d $(BIN_DIR)
	install -m 755 $(TARGET_DIR)/$(BINARY_NAME) $(BIN_DIR)/$(BINARY_NAME)
	install -m 755 $(TARGET_DIR)/$(DAEMON_BINARY_NAME) $(BIN_DIR)/$(DAEMON_BINARY_NAME)
	install -d /etc/systemd/system
	install -m 644 nvsleepifyd.service /etc/systemd/system/nvsleepifyd.service
	install -d $(DBUS_CONF_DIR)
	install -m 644 org.nvsleepify.conf $(DBUS_CONF_DIR)/org.nvsleepify.conf
	systemctl daemon-reload
	systemctl reload dbus || true
	@echo "Installed $(BINARY_NAME) and $(DAEMON_BINARY_NAME)"

uninstall:
	systemctl disable --now nvsleepifyd.service || true
	rm -f $(BIN_DIR)/$(BINARY_NAME)
	rm -f $(BIN_DIR)/$(DAEMON_BINARY_NAME)
	rm -f /etc/systemd/system/nvsleepifyd.service
	rm -f $(DBUS_CONF_DIR)/org.nvsleepify.conf
	systemctl daemon-reload
	systemctl reload dbus || true
	@echo "Uninstalled $(BINARY_NAME)"

clean:
	cargo clean
