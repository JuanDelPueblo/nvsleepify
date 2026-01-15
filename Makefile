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
	@echo "Installed $(BINARY_NAME) to $(BIN_DIR)"

uninstall:
	rm -f $(BIN_DIR)/$(BINARY_NAME)
	@echo "Uninstalled $(BINARY_NAME) from $(BIN_DIR)"

clean:
	cargo clean
