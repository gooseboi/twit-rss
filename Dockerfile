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

FROM debian:bookworm-20231009-slim as firefox_builder

# Install necessary packages
RUN set -eux; \
	apt-get update; \
	apt-get install -y --no-install-recommends \
		curl ca-certificates bzip2;

# Download the geckodriver release archive
RUN set -eux; \
	curl --location --fail \
		"https://github.com/mozilla/geckodriver/releases/download/v0.33.0/geckodriver-v0.33.0-linux64.tar.gz" \
		--output geckodriver-linux64.tar.gz;

# Extract the geckodriver binary to /
RUN set -eux; \
	mkdir /geckodriver; \
	tar xzvf geckodriver-linux64.tar.gz -C /geckodriver;

# Download the firefox release archive
RUN set -eux; \
	curl --location --fail \
		"https://archive.mozilla.org/pub/firefox/releases/115.3.1esr/linux-x86_64/en-US/firefox-115.3.1esr.tar.bz2" \
		--output firefox-linux64.tar.bz2;

# Extract the firefox release archive
RUN set -eux; \
	mkdir /firefox; \
	tar xjvf firefox-linux64.tar.bz2 -C /firefox;

# Copy everything into bin for easy PATH setting
RUN set -eux; \
	rm -rfv /root/bin; \
	cp -rv /firefox/firefox /root; \
	cp -v /geckodriver/geckodriver /root/firefox;

#########################################################################

FROM debian:bookworm-20231009-slim

# Download the necessary dependencies
RUN set -eux; \
	apt-get update; \
	apt-get install -y --no-install-recommends \
		libssl-dev; \
	apt-get clean;

# Copy firefox and geckodriver into the image
WORKDIR /root
COPY --from=firefox_builder /root/firefox ./firefox

# Set path for firefox and geckodriver
ENV PATH="/root/firefox:$PATH"

WORKDIR /app
COPY --from=build /app/target/release/twitarc .

ENV TWITARC_DATA="/data"

ENTRYPOINT ["/app/twitarc"]
