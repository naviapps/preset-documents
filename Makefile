.DEFAULT_GOAL := check
SWIFT_FLAGS := -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
.PHONY: check check-source test-managed check-distribution lint native native-verify test test-consumer test-installed-consumers test-rust test-tools test-bindings docs
check: check-source test test-consumer

check-source: lint test-rust test-managed
check-distribution: test-tools test-bindings test-installed-consumers docs
lint:
	mise run lint
native:
	python3 tools/native.py
native-verify:
	python3 tools/native.py --verify
test:
	swift test $(SWIFT_FLAGS)
test-consumer:
	mise run sdk-tooling -- swift-consumer
test-installed-consumers:
	mise run sdk-tooling -- installed-consumers
test-tools:
	python3 -B -m unittest discover -s tools/tests -v
test-rust:
	cargo clippy --workspace --all-targets --locked --offline -- -D warnings
	cargo test --workspace --locked --offline
test-bindings:
	python3 tools/bindings.py

.PHONY: format
format:
	mise run format

docs:
	mise run sdk-tooling -- docs

test-managed:
	PRESET_DOCUMENTS_NATIVE_LIBRARY="$(CURDIR)/Artifacts/CPresetDocuments.xcframework/macos-arm64_x86_64/CPresetDocuments.framework/CPresetDocuments" \
		dotnet run --configuration Release --project bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj -- --depth-contracts
