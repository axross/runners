FROM ghcr.io/actions/actions-runner:2.337.0@sha256:e5496277be5d09bc968b3d64911b74e219ac4a3f2edce956a3ecf9271bea1ef4

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

# the tool cache and the per-tool cache directories are the mount points of the
# host's cache volumes. creating them owned by runner keeps a fresh volume
# writable.
RUN mkdir -p /opt/hostedtoolcache \
        /home/runner/.cargo \
        /home/runner/.rustup \
        /home/runner/.npm \
    && chown runner:docker /opt/hostedtoolcache \
        /home/runner/.cargo \
        /home/runner/.rustup \
        /home/runner/.npm

# hadolint ignore=DL3066
USER runner
