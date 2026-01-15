PREFIX ?= /usr/local
BIN_DIR = $(PREFIX)/bin
BINARY_NAME = nvsleepify
TARGET_DIR = target/release

.PHONY: all build install uninstall clean

all: build

build:
	cargo build --release

install: build
	install -d $(BIN_DIR)
	install -m 755 $(TARGET_DIR)/$(BINARY_NAME) $(BIN_DIR)/$(BINARY_NAME)
	install -d /etc/systemd/system
	install -m 644 nvsleepify.service /etc/systemd/system/nvsleepify.service
	systemctl daemon-reload
	@echo "Installed $(BINARY_NAME) to $(BIN_DIR) and service file"

uninstall:
	systemctl disable --now nvsleepify.service || true
	rm -f $(BIN_DIR)/$(BINARY_NAME)
	rm -f /etc/systemd/system/nvsleepify.service
	systemctl daemon-reload
	@echo "Uninstalled $(BINARY_NAME) from $(BIN_DIR)"

clean:
	cargo clean
