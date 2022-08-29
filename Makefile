#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

V ?= 0
Q = @
ifneq ($V,0)
	Q =
endif

JOBS ?= 8

OUTPUT_DIR ?= out

BUILD_TYPES := release debug

GN_ARCHS     := arm arm64 x86 x64

ANDROID_TARGETS := $(foreach t, $(BUILD_TYPES),     \
			$(foreach a, $(GN_ARCHS),   \
				android/$(a)/$(t)))

IOS_TARGETS := ios/release

# This can be overridden on the command line, e.g. "make electron NODEJS_ARCH=ia32"
# Note: make sure to only use NodeJS architectures here, like x64, ia32, arm64, etc.
NODEJS_ARCH := x64

help:
	$(Q) echo "The following build targets are supported:"
	$(Q) echo "  ios      -- download WebRTC and build for the iOS platform"
	$(Q) echo "  android  -- download WebRTC and build for the Android platform"
	$(Q) echo "  electron -- build an Electron library"
	$(Q) echo "  cli      -- build the test cli (1:1 calls)"
	$(Q) echo "  gctc     -- build the test cli (group calls)"
	$(Q) echo
	$(Q) echo "For the electon/cli/gctc builds, you can specify an optional platform"
	$(Q) echo "which will download WebRTC. For example:"
	$(Q) echo "  $ make electron PLATFORM=unix"
	$(Q) echo
	$(Q) echo "The following clean targets are supported:"
	$(Q) echo "  clean     -- remove all build artifacts"
	$(Q) echo "  distclean -- remove everything"
	$(Q) echo

android: $(ANDROID_TARGETS)
	$(Q) ./bin/build-aar -j$(JOBS)

$(OUTPUT_DIR)/android.env:
	$(Q) echo "Preparing Android workspace"
	$(Q) ./bin/prepare-workspace android

android/%: ARCH = $(word 1, $(subst /, , $*))
android/%: TYPE = $(word 2, $(subst /, , $*))
android/%: $(OUTPUT_DIR)/android.env
	$(Q) ./bin/build-aar --compile-only --$(TYPE)-build --arch $(ARCH) -j$(JOBS)

ios: $(IOS_TARGETS)

$(OUTPUT_DIR)/ios.env:
	$(Q) echo "Preparing iOS workspace"
	$(Q) ./bin/prepare-workspace ios

ios/%: TYPE = $*
ios/%: $(OUTPUT_DIR)/ios.env
	$(Q) if [ "$(TYPE)" = "debug" ] ; then \
		echo "iOS: Debug build" ; \
		./bin/build-ios -d ; \
	else \
		echo "iOS: Release build" ; \
		./bin/build-ios ; \
	fi

electron:
	$(Q) if [ "$(PLATFORM)" != "" ] ; then \
		echo "Electron: Preparing workspace for $(PLATFORM)" ; \
		./bin/prepare-workspace $(PLATFORM) ; \
	fi
	$(Q) if [ "$(TYPE)" = "debug" ] ; then \
		echo "Electron: Debug build" ; \
		TARGET_ARCH=$(NODEJS_ARCH) ./bin/build-electron -d ; \
	else \
		echo "Electron: Release build" ; \
		TARGET_ARCH=$(NODEJS_ARCH) ./bin/build-electron -r ; \
	fi
	$(Q) (cd src/node && yarn install && yarn build)

cli:
	$(Q) if [ "$(PLATFORM)" != "" ] ; then \
		echo "cli: Preparing workspace for $(PLATFORM)" ; \
		./bin/prepare-workspace $(PLATFORM) ; \
	fi
	$(Q) if [ "$(TYPE)" = "release" ] ; then \
		echo "cli: Release build" ; \
		./bin/build-cli -r ; \
	else \
		echo "cli: Debug build" ; \
		./bin/build-cli -d ; \
	fi

gctc:
	$(Q) if [ "$(PLATFORM)" != "" ] ; then \
		echo "gctc: Preparing workspace for $(PLATFORM)" ; \
		./bin/prepare-workspace $(PLATFORM) ; \
	fi
	$(Q) if [ "$(TYPE)" = "release" ] ; then \
		echo "gctc: Release build" ; \
		./bin/build-gctc -r ; \
	else \
		echo "gctc: Debug build" ; \
		./bin/build-gctc -d ; \
	fi

PHONY += clean
clean:
	$(Q) ./bin/build-aar --clean
	$(Q) ./bin/build-cli --clean
	$(Q) ./bin/build-gctc --clean
	$(Q) ./bin/build-electron --clean
	$(Q) ./bin/build-ios --clean
	$(Q) rm -rf ./src/webrtc/src/out

PHONY += distclean
distclean:
	$(Q) rm -rf ./out
	$(Q) rm -rf ./src/rust/target
	$(Q) rm -rf ./src/node/build
	$(Q) rm -rf ./src/node/dist
	$(Q) rm -rf ./src/node/node_modules
	$(Q) rm -rf ./src/webrtc/src/out

.PHONY: $(PHONY)
