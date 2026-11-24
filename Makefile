SDK_TOOLING ?= mise run sdk-tooling --

.DEFAULT_GOAL := check
SWIFT_FLAGS := -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
.PHONY: check check-source test-managed check-distribution lint native native-verify check-header generate-header test test-consumer test-installed-consumers test-rust test-distribution-tools test-bindings clean
check: check-source test test-consumer

check-source: lint check-header test-rust test-managed
check-distribution: check-header test-distribution-tools test-bindings test-installed-consumers
lint:
	$(SDK_TOOLING) quality check --dotnet
native: generate-header
	$(SDK_TOOLING) xcframework
native-verify: check-header
	$(SDK_TOOLING) xcframework --verify
check-header:
	$(SDK_TOOLING) c-headers check
generate-header:
	$(SDK_TOOLING) c-headers generate
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

test-managed:
	dotnet run --configuration Release --project bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj \
		-p:PresetDocumentsNativeLibrary="$(CURDIR)/Artifacts/CPresetDocuments.xcframework/macos-arm64_x86_64/CPresetDocuments.framework/CPresetDocuments"

clean:
	$(SDK_TOOLING) clean target .build .native-build \
		bindings/dotnet/src/NaviApps.PresetDocuments/bin \
		bindings/dotnet/src/NaviApps.PresetDocuments/obj \
		bindings/dotnet/tests/PresetDocuments.Tests/bin \
		bindings/dotnet/tests/PresetDocuments.Tests/obj
