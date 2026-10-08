FROM ghcr.io/actions/actions-runner:2.337.0@sha256:e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4 AS base

FROM base AS android-ndks
ENV ANDROID_SDK_ROOT=/home/runner/.android/sdk
COPY install-ndks.sh /tmp/install-ndks.sh
RUN mkdir -p "$ANDROID_SDK_ROOT/ndk" && bash /tmp/install-ndks.sh

FROM base AS android-tools
# the standard Java cache is outside the runner's home and needs root during
# provisioning; only selected tool directories cross into the final image.
# hadolint ignore=DL3066
USER root
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV ANDROID_SDK_ROOT=/home/runner/.android/sdk \
    RUNNER_TOOL_CACHE=/opt/hostedtoolcache

RUN mkdir -p "$RUNNER_TOOL_CACHE" "$ANDROID_SDK_ROOT" /home/runner/.gradle/wrapper/dists \
    && if [ "$(uname -m)" != x86_64 ]; then echo "Preinstalled Android tools are available on Linux x64 only."; exit 0; fi \
    && set -eu \
    && work_dir="$(mktemp -d)" \
    && trap 'rm -rf -- "$work_dir"' EXIT \
    && java_home="$RUNNER_TOOL_CACHE/Java_Temurin-Hotspot_jdk/17.0.20-101/x64" \
    && curl -fsSL 'https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.20.1%2B1/OpenJDK17U-jdk_x64_linux_hotspot_17.0.20.1_1.tar.gz' -o "$work_dir/temurin.tar.gz" \
    && printf '%s  %s\n' '3808d1d15e3ec6bd5b84057fb5d84c33d8a1536a258146bcea2e603fc726e08e' "$work_dir/temurin.tar.gz" | sha256sum --check --strict \
    && mkdir -p "$java_home" \
    && tar -xzf "$work_dir/temurin.tar.gz" --strip-components=1 -C "$java_home" \
    && grep -Fx 'JAVA_RUNTIME_VERSION="17.0.20.1+1"' "$java_home/release" \
    && "$java_home/bin/java" -version \
    && touch "$java_home.complete"

RUN set -eu; \
    if [ "$(uname -m)" != x86_64 ]; then exit 0; fi; \
    work_dir="$(mktemp -d)"; \
    trap 'rm -rf -- "$work_dir"' EXIT; \
    repository_url=https://dl.google.com/android/repository; \
    printf '%s\n' \
        'commandlinetools-linux-14742923_latest.zip 04453066b540409d975c676d781da1477479dde3761310f1a7eb92a1dfb15af7 cmdline-tools/20.0 cmdline-tools 20.0' \
        'platform-36_r02.zip 37607369a28c5b640b3a7998868d45898ebcb777565a0e85f9acf36f29631d2e platforms/android-36 android-36 2' \
        'build-tools_r35_linux.zip bd3a4966912eb8b30ed0d00b0cda6b6543b949d5ffe00bea54c04c81e1561d88 build-tools/35.0.0 android-15 35.0.0' \
        'build-tools_r36_linux.zip 5d9ac77fb6ff43d9da518a337b4fcf8f9097113df531d99ccefe80ef7ce8250b build-tools/36.0.0 android-16 36.0.0' \
        'cmake-3.22.1-linux.zip 9196644852a978012caf7a4067ba1898debf6cc204c3341562771e31080d6869 cmake/3.22.1 . 3.22.1' \
        'platform-tools_r37.0.1-linux.zip d230f13842f60f782a8645f9c813f8f845bf36089ea7289f28c48f17979313f1 platform-tools platform-tools 37.0.1' \
    | while read -r filename checksum destination archive_root revision; do \
        archive="$work_dir/$filename"; \
        curl -fsSL "$repository_url/$filename" -o "$archive"; \
        printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --strict; \
        mkdir -p "$work_dir/extracted"; \
        unzip -q "$archive" -d "$work_dir/extracted"; \
        package="$work_dir/extracted/$archive_root"; \
        actual_revision="$(sed -n 's/^Pkg.Revision[[:space:]]*=[[:space:]]*//p' "$package/source.properties")"; \
        if [ "$actual_revision" != "$revision" ]; then \
            echo "SDK revision mismatch for $destination: expected $revision, found $actual_revision" >&2; exit 1; \
        fi; \
        case "$destination" in \
            cmdline-tools/20.0) package_path='cmdline-tools;20.0'; xml_revision='<major>20</major><minor>0</minor>'; display_name='Android SDK Command-line Tools';; \
            cmake/3.22.1) package_path='cmake;3.22.1'; xml_revision='<major>3</major><minor>22</minor><micro>1</micro>'; display_name='CMake 3.22.1';; \
            *) package_path='';; \
        esac; \
        if [ -n "$package_path" ]; then \
            printf '%s\n' \
                '<?xml version="1.0" encoding="UTF-8"?>' \
                '<common:repository xmlns:common="http://schemas.android.com/repository/android/common/02" xmlns:generic="http://schemas.android.com/repository/android/generic/02" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">' \
                "<localPackage path=\"$package_path\" obsolete=\"false\">" \
                '<type-details xsi:type="generic:genericDetailsType" />' \
                "<revision>$xml_revision</revision>" \
                "<display-name>$display_name</display-name>" \
                '</localPackage></common:repository>' > "$package/package.xml"; \
        fi; \
        mkdir -p "$ANDROID_SDK_ROOT/$destination"; \
        mv "$package/"* "$ANDROID_SDK_ROOT/$destination/"; \
        rm -rf -- "$archive" "$work_dir/extracted"; \
    done; \
    grep -Fx 'AndroidVersion.ApiLevel=36' "$ANDROID_SDK_ROOT/platforms/android-36/source.properties"

RUN if [ "$(uname -m)" != x86_64 ]; then exit 0; fi \
    && set -eu \
    && distribution_dir=/home/runner/.gradle/wrapper/dists/gradle-9.3.1-bin/23ovyewtku6u96viwx3xl3oks \
    && mkdir -p "$distribution_dir" \
    && archive="$distribution_dir/gradle-9.3.1-bin.zip" \
    && curl -fsSL https://services.gradle.org/distributions/gradle-9.3.1-bin.zip -o "$archive" \
    && printf '%s  %s\n' 'b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06' "$archive" | sha256sum --check --strict \
    && unzip -tq "$archive" \
    && unzip -Z1 "$archive" | grep -Fx 'gradle-9.3.1/lib/gradle-launcher-9.3.1.jar'

# hadolint ignore=DL3066
USER runner

FROM base

# the base image ends as the unprivileged runner user; root is needed only for
# the package install and directory setup below. the base image names its user
# and publishes no numeric id, so the name is the only stable reference.
# hadolint ignore=DL3066
USER root

# every package a job assumes is present must be installed here: the package
# index is removed in the same layer, so a job-time apt-get install finds none.
# versions are not pinned because the noble archive replaces revisions and a
# pinned revision would fail the build once it is superseded.
# hadolint ignore=DL3008
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ccache \
        libyaml-0-2 \
        libgmp10 \
    && rm -rf /var/lib/apt/lists/*

# ruby/setup-ruby resolves its prebuilt Rubies to this fixed path, so the tool
# cache lives here rather than under the runner work directory.
ENV RUNNER_TOOL_CACHE=/opt/hostedtoolcache

ENV ANDROID_HOME=/home/runner/.android/sdk \
    ANDROID_SDK_ROOT=/home/runner/.android/sdk
COPY --from=android-ndks --chown=runner:docker /home/runner/.android/sdk /home/runner/.android/sdk
COPY --from=android-tools --chown=runner:docker /home/runner/.android/sdk /home/runner/.android/sdk
COPY --from=android-tools --chown=runner:docker /opt/hostedtoolcache /opt/hostedtoolcache
COPY --from=android-tools --chown=runner:docker /home/runner/.gradle /home/runner/.gradle

# runner ownership keeps fresh tool-cache and language-cache volumes writable.
# setup-android also writes metadata outside the SDK root, so .android and its
# download cache need the same ownership within each disposable container.
RUN mkdir -p /opt/hostedtoolcache \
        /home/runner/.android/cache \
        /home/runner/.cargo \
        /home/runner/.rustup \
        /home/runner/.npm \
    && chown runner:docker /opt/hostedtoolcache \
        /home/runner/.android \
        /home/runner/.android/cache \
        /home/runner/.cargo \
        /home/runner/.rustup \
        /home/runner/.npm

COPY runner-diagnostics.cpp /tmp/runner-diagnostics.cpp
RUN g++ -std=c++17 -O2 -Wall -Wextra -Werror /tmp/runner-diagnostics.cpp -o /usr/local/bin/runner-diagnostics \
    && rm /tmp/runner-diagnostics.cpp

# hadolint ignore=DL3066
USER runner
