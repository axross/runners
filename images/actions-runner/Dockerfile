FROM ghcr.io/actions/actions-runner:2.337.0@sha256:e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4 AS base

FROM base AS android-ndks
ENV ANDROID_SDK_ROOT=/home/runner/.android/sdk
COPY install-ndks.sh /tmp/install-ndks.sh
RUN mkdir -p "$ANDROID_SDK_ROOT/ndk" && bash /tmp/install-ndks.sh

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

# setup-android writes download metadata outside the SDK root. Android paths
# stay container-local; the other directories can serve the host's cache mounts.
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

# hadolint ignore=DL3066
USER runner
