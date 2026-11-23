.DEFAULT_GOAL := check
SWIFT_FLAGS := -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
.PHONY: check lint native native-verify test test-consumer test-installed-consumers test-rust test-tools test-bindings docs
check: lint test-tools test-rust test-bindings test test-consumer test-installed-consumers docs
lint:
	mise exec -- python3 -B tools/quality.py check --dotnet
native:
	python3 tools/native.py
native-verify:
	python3 tools/native.py --verify
test:
	swift test $(SWIFT_FLAGS)
test-consumer:
	python3 -B tools/consumer.py
test-installed-consumers:
	python3 -B tools/install-consumers.py
test-tools:
	python3 -B -m unittest discover -s tools/tests -v
test-rust:
	cargo clippy --workspace --all-targets --locked --offline -- -D warnings
	cargo test --workspace --locked --offline
test-bindings:
	python3 tools/bindings.py

.PHONY: format
format:
	mise exec -- python3 -B tools/quality.py format --dotnet

docs:
	python3 -B tools/docs.py
