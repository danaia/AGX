.PHONY: prepare configure dependencies build check-image
prepare configure dependencies build:
	./scripts/build.sh $@
check-image:
	python3 scripts/check-image.py output/images

.PHONY: tui-test tui-build deploy-tui
tui-test:
	./scripts/cargo.sh test --workspace --locked
tui-build:
	./scripts/cargo.sh build --locked --release --target aarch64-unknown-linux-gnu --package agx-cli
deploy-tui: tui-build
	./scripts/deploy-tui.sh

.PHONY: network-build deploy-network
network-build:
	./scripts/cargo.sh build --locked --workspace --release --target aarch64-unknown-linux-gnu
deploy-network: network-build
	./scripts/deploy-tui.sh
	./scripts/deploy-network.sh

.PHONY: deploy-profile
deploy-profile: network-build
	./scripts/deploy-profile.sh

.PHONY: system-update-package
system-update-package: build check-image
	python3 scripts/create-system-update.py

.PHONY: stage-system-update
stage-system-update:
	./scripts/stage-system-update.sh

.PHONY: release-update
release-update:
	./scripts/create-release-update.sh

.PHONY: init-build deploy-init
init-build:
	./scripts/cargo.sh build --locked --workspace --release --target aarch64-unknown-linux-gnu
deploy-init: init-build
	./scripts/deploy-init.sh
