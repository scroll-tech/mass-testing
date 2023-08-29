FROM scrolltech/go-rust-builder as builder

RUN mkdir -p /root/src
ADD . /root/src
RUN cargo build --release --bin testnet-runner
RUN cd /root/src/target/release && find -name libzktrie.so | xargs -I {} cp {} /root/src/target/release

FROM builder

COPY --from=builder /root/src/target/release/testnet-runner /bin/
COPY --from=builder /root/src/run-testnet/run.sh /bin/testnet-runner.sh
COPY --from=builder /root/src/target/release/libzktrie.so /usr/local/lib
ENV LD_LIBRARY_PATH /usr/local/lib

CMD testnet-runner.sh
