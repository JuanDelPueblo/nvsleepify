PREFIX ?= /usr/local
BIN_DIR = $(PREFIX)/bin
BINARY_NAME = nvsleepify
DAEMON_BINARY_NAME = nvsleepifyd
TARGET_DIR = target/release

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
	systemctl daemon-reload
	@echo "Installed $(BINARY_NAME) and $(DAEMON_BINARY_NAME) to $(BIN_DIR) and service files"

uninstall:
	systemctl disable --now nvsleepifyd.service || true
	rm -f $(BIN_DIR)/$(BINARY_NAME)
	rm -f $(BIN_DIR)/$(DAEMON_BINARY_NAME)
	rm -f /etc/systemd/system/nvsleepifyd.service
	systemctl daemon-reload
	@echo "Uninstalled $(BINARY_NAME) from $(BIN_DIR)"

clean:
	cargo clean
