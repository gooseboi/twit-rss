FROM debian:bookworm-20231009-slim as build

# Install curl and deps
RUN set -eux; \
	apt-get update; \
	apt-get install -y --no-install-recommends \
		curl ca-certificates gcc libc6-dev pkg-config libssl-dev;

# Install rustup
# We don't really care what toolchain it installs, as we just use
# rust-toolchain.toml, but as far as I know there is no way to just install
# the toolchain in the file at this point
RUN set -eux; \
		curl --location --fail \
			"https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init" \
			--output /rustup-init; \
		chmod +x /rustup-init; \
		/rustup-init -y --no-modify-path; \
		rm /rustup-init;

# Add rustup to path, check that it works, and set profile to minimal
ENV PATH=${PATH}:/root/.cargo/bin
RUN set -eux; \
		rustup --version; \
		rustup set profile minimal;

# Copy sources and build them
WORKDIR /app
COPY src src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./

RUN --mount=type=cache,target=/root/.rustup \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
	set -eux; \
	cargo build --release

#########################################################################

# TODO: Add firefox and geckodriver

FROM debian:bookworm-20231009-slim

WORKDIR /app
COPY --from=build /app/target/release/twitarc .

ENV TWITARC_DATA="/data"

ENTRYPOINT ["/app/twitarc"]
