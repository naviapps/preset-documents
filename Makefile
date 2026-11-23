SDK_TOOLING ?= mise run sdk-tooling --

.DEFAULT_GOAL := check
SWIFT_FLAGS := -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
.PHONY: check check-source test-managed check-distribution lint native native-verify test test-consumer test-installed-consumers test-rust test-distribution-tools test-bindings docs
check: check-source test test-consumer

check-source: lint test-rust test-managed
check-distribution: test-distribution-tools test-bindings test-installed-consumers docs
lint:
	$(SDK_TOOLING) quality check --dotnet
native:
	$(SDK_TOOLING) xcframework
native-verify:
	$(SDK_TOOLING) xcframework --verify
test:
	swift test $(SWIFT_FLAGS)
test-consumer:
	$(SDK_TOOLING) swift-consumer
test-installed-consumers:
	$(SDK_TOOLING) rust-c-consumers
test-distribution-tools:
	$(SDK_TOOLING) python -m unittest discover -s distribution/tests -v
test-rust:
	cargo clippy --workspace --all-targets --locked --offline -- -D warnings
	cargo test --workspace --locked --offline
test-bindings:
	$(SDK_TOOLING) python distribution/verify_bindings.py

.PHONY: format
format:
	$(SDK_TOOLING) quality format --dotnet

docs:
	$(SDK_TOOLING) docs

test-managed:
	PRESET_DOCUMENTS_NATIVE_LIBRARY="$(CURDIR)/Artifacts/CPresetDocuments.xcframework/macos-arm64_x86_64/CPresetDocuments.framework/CPresetDocuments" \
		dotnet run --configuration Release --project bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj
