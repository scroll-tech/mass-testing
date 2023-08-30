FROM scrolltech/go-rust-builder as builder

RUN mkdir -p /root/src
ADD . /root/src
RUN <<EOF bash
pushd /root/src
cargo build --release --bin testnet-runner
pushd ./target/release
find -name libzktrie.so | xargs -I {} cp {} ./
popd
popd
EOF

FROM ubuntu:20.04
 
RUN apt update && apt install -y curl
COPY --from=builder /root/src/target/release/testnet-runner /bin/
COPY --from=builder /root/src/run.sh /bin/testnet-runner.sh
COPY --from=builder /root/src/target/release/libzktrie.so /usr/local/lib
ENV LD_LIBRARY_PATH /usr/local/lib
RUN mkdir issues

CMD testnet-runner.sh
