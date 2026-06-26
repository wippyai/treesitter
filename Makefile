.PHONY: help image build-component wippy lint unit-test test check clean

RUNTIME_DIR ?= ../runtime
OS    := $(shell uname -s | tr '[:upper:]' '[:lower:]')
ARCH  := $(shell uname -m | sed -e 's/x86_64/amd64/' -e 's/aarch64/arm64/' -e 's/arm64/arm64/')
WIPPY_BIN := $(RUNTIME_DIR)/dist/wippy-$(OS)-$(ARCH)
IMAGE := treesitter-wasi:33

help:
	@echo "treesitter make targets:"
	@echo "  image            build the wasi-sdk + rust build image"
	@echo "  build-component  build the Rust WASM engine in docker, stage it, inject sha256"
	@echo "  wippy            point ./wippy at the runtime binary"
	@echo "  lint             ./wippy lint --level error"
	@echo "  unit-test        cargo test (native)"
	@echo "  test             ./wippy run test"
	@echo "  check            build-component + lint + test"
	@echo "  clean            remove build artifacts"

image:
	docker build -t $(IMAGE) component

build-component:
	docker run --rm -v $(CURDIR):/work $(IMAGE) bash component/build.sh
	python3 scripts/stage_hashes.py

wippy:
	@ln -sf $(WIPPY_BIN) ./wippy
	@./wippy version

lint:
	./wippy lint --level error

unit-test:
	cd component && cargo test

test:
	./wippy run test

check: build-component lint test

clean:
	rm -rf component/target dist
