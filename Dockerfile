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
WORKDIR /opt
COPY --from=builder /root/src/target/release/testnet-runner /bin/
COPY --from=builder /root/src/target/release/libzktrie.so /usr/local/lib
COPY --from=builder /root/src/run.sh /opt/testnet-runner.sh
COPY --from=builder /root/src/wrap_group.sh /opt/
ENV LD_LIBRARY_PATH /usr/local/lib
RUN mkdir issues

CMD ./testnet-runner.sh
