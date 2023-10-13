FROM scrolltech/cuda-go-rust-builder:cuda-11.7.1-go-1.19-rust-nightly-2022-12-10 as builder

ENV LD_LIBRARY_PATH /usr/local/cuda/lib64:$LD_LIBRARY_PATH
RUN mkdir -p /root/src
ADD . /root/src
RUN cd /root/src && mkdir .cargo && echo 'paths = ["/root/src/halo2-gpu/halo2_proofs"]' > .cargo/config
RUN cd /root/src && cargo build --release --bin testnet-runner
RUN cd /root/src/target/release && find -name libzktrie.so | xargs -I {} cp {} ./

FROM nvidia/cuda:11.7.1-runtime-ubuntu22.04

RUN apt update && apt install -y curl
WORKDIR /opt
RUN mkdir assets
RUN curl -o ./assets/layer1.config https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/layer1.config
RUN curl -o ./assets/layer2.config https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/layer2.config
RUN curl -o ./assets/layer3.config https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/layer3.config
RUN curl -o ./assets/layer4.config https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/layer4.config
RUN curl -o ./assets/agg_vk.vkey https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/agg_vk.vkey
RUN curl -o ./assets/chunk.protocol https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/chunk.protocol
RUN curl -o ./assets/evm_verifier.bin https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.9.7/evm_verifier.bin
COPY --from=builder /root/src/target/release/testnet-runner /bin/
COPY --from=builder /root/src/target/release/libzktrie.so /usr/local/lib
COPY --from=builder /root/src/run.sh /opt/testnet-runner.sh
ENV LD_LIBRARY_PATH /usr/local/cuda/lib64:/usr/local/lib:$LD_LIBRARY_PATH
RUN mkdir issues

CMD ./testnet-runner.sh
