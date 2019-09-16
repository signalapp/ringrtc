
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
	$(Q) echo "Please choose one of the following targets: android, ios"
	$(Q) exit 1

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
