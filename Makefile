
V ?= 0
Q = @
ifneq ($V,0)
	Q =
endif

JOBS ?= 8

BUILD_TYPES := release debug

GN_ARCHS     := arm arm64 x86 x64

ANDROID_TARGETS := $(foreach t, $(BUILD_TYPES),     \
			$(foreach a, $(GN_ARCHS),   \
				android/$(a)/$(t)))

IOS_TARGETS := ios/release

help:
	$(Q) echo "The following build targets are supported:"
	$(Q) echo "  ios     -- download WebRTC and build iOS platform products."
	$(Q) echo "  android -- download WebRTC and build Android platform products."
	$(Q) echo
	$(Q) echo "The following clean targets are supported:"
	$(Q) echo "  clean     -- remove all platform build products."
	$(Q) echo "  distclean -- remove everything, including downloaded WebRTC dependencies."
	$(Q) echo

android: $(ANDROID_TARGETS)
	$(Q) ./bin/build-aar -j$(JOBS)

out/android.env:
	$(Q) echo "Preparing Android workspace"
	$(Q) ./bin/prepare-workspace android

android/%: ARCH = $(word 1, $(subst /, , $*))
android/%: TYPE = $(word 2, $(subst /, , $*))
android/%: out/android.env
	$(Q) ./bin/build-aar --compile-only --$(TYPE)-build --arch $(ARCH) -j$(JOBS)

ios: $(IOS_TARGETS)

out/ios.env:
	$(Q) echo "Preparing iOS workspace"
	$(Q) ./bin/prepare-workspace ios

ios/%: TYPE = $*
ios/%: out/ios.env
	$(Q) if [ "$(TYPE)" = "debug" ] ; then \
		echo "iOS: Debug build" ; \
		./bin/build-ios -d ; \
	else \
		echo "iOS: Release build" ; \
		./bin/build-ios ; \
	fi

electron:
	# TODO ./bin/prepare-workspace
	$(Q) ./bin/build-electron
	$(Q) (cd src/node && yarn install && yarn build)

cli:
	# TODO: ./bin/prepare-workspace
	$(Q) ./bin/build-cli

PHONY += clean
clean:
	$(Q) ./bin/build-aar --clean
	$(Q) ./bin/build-ios --clean
	$(Q) rm -rf ./src/webrtc/src/out

PHONY += distclean
distclean:
	$(Q) rm -rf ./out
	$(Q) rm -rf ./src/webrtc/src/out

.PHONY: $(PHONY)
