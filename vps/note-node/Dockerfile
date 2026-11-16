# syntax=docker/dockerfile:1
# Note ("Notebc Core" / notecoind, a Bitcoin Core 0.16.3 fork) built from the
# pinned source of release v1.1.0.0 — no pre-built binary is trusted.
# Ubuntu 18.04 because the 0.16 code base needs its Boost/OpenSSL generation;
# the container publishes no port and only talks to peers and the backend.
FROM ubuntu:18.04 AS build
ARG NOTE_TAG=v1.1.0.0
ARG NOTE_COMMIT=61fa8bfb68d53a744ffe648fcf75b101a3f8d893
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential libtool autotools-dev automake pkg-config bsdmainutils python3 \
      git ca-certificates curl wget \
      libssl-dev libevent-dev libboost-system-dev libboost-filesystem-dev \
      libboost-chrono-dev libboost-test-dev libboost-thread-dev libboost-program-options-dev \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch "${NOTE_TAG}" https://github.com/note-llc/NoteBlockchain.git /src \
    && cd /src && test "$(git rev-parse HEAD)" = "${NOTE_COMMIT}"
WORKDIR /src
# Berkeley DB 4.8 (wallet format) from the repo's own helper, which verifies
# the tarball checksum.
RUN chmod +x contrib/install_db4.sh autogen.sh share/genbuild.sh 2>/dev/null; ./contrib/install_db4.sh /src
RUN ./autogen.sh \
    && ./configure --without-gui --disable-tests --disable-bench --without-miniupnpc \
         BDB_LIBS="-L/src/db4/lib -ldb_cxx-4.8" BDB_CFLAGS="-I/src/db4/include" \
    && make -j"$(nproc)" \
    && strip src/notecoind src/notecoin-cli

FROM ubuntu:18.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl unzip \
      libssl1.1 libevent-2.1-6 libevent-pthreads-2.1-6 \
      libboost-system1.65.1 libboost-filesystem1.65.1 libboost-chrono1.65.1 \
      libboost-thread1.65.1 libboost-program-options1.65.1 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10002 note && useradd --uid 10002 --gid 10002 --home-dir /data --no-create-home --shell /usr/sbin/nologin note \
    && mkdir -p /data && chown note:note /data
COPY --from=build /src/src/notecoind /src/src/notecoin-cli /usr/local/bin/
COPY entrypoint.sh /usr/local/bin/note-entrypoint.sh
RUN chmod 0755 /usr/local/bin/note-entrypoint.sh
USER note
VOLUME /data
EXPOSE 9332
ENTRYPOINT ["/usr/local/bin/note-entrypoint.sh"]
