# The build process consists of two primary steps
# 1. Install all the dependencies & build the binary
# 2. Copy the binary into a fresh non-polluted docker image
# cargo-chef is used to cache the rust dependencies (listed in Cargo.toml) so they're not rebuild over and over across builds

#----
FROM lukemathwalker/cargo-chef:latest-rust-1.84.0 as chef
WORKDIR /app
#----

#----
# build a plan
FROM chef as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json
#----

#----
FROM chef as builder
COPY --from=planner /app/recipe.json recipe.json

# set the build profile to release by default
ARG BUILD_PROFILE=release
ENV BUILD_PROFILE $BUILD_PROFILE
ENV SQLX_OFFLINE true

RUN apt update && apt install lld clang -y
RUN cargo chef cook --profile=$BUILD_PROFILE --recipe-path recipe.json

COPY . .
RUN cargo build --profile=$BUILD_PROFILE --locked --bin tx-fees

# determine the correct target directory
RUN if [ "$BUILD_PROFILE" = "dev" ]; then \
    cp /app/target/debug/tx-fees /app/tx-fees; \
    else \
    cp /app/target/$BUILD_PROFILE/tx-fees /app/tx-fees; \
    fi
#----

#----
FROM debian:12 AS runtime
# without curl it hangs on the wss handshake +
# error while loading shared libraries: libssl.so.3
RUN apt update && apt install -y curl libssl-dev pkg-config
WORKDIR /app

# copy the binary from the build stage
COPY --from=builder /app/tx-fees /app

# note:
#   normally you wouldn't want to copy the migrations into the image itself
#   I've approached it this way to simplify the project startup process as much as possible
COPY ./migrations/ /app/migrations

EXPOSE 8080
ENTRYPOINT ["/app/tx-fees"]
#----
