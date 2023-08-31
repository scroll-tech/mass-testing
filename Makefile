.PHONY: download-setup docker-build gpu-docker-build

HALO2_GPU_VERSION=v0.5.0

docker-build:
	docker build -t scrolltech/testnet-runner .

gpu-download-halo2:
	git clone -b ${HALO2_GPU_VERSION} git@github.com:scroll-tech/halo2-gpu.git

gpu-docker-build:
	docker build -f Dockerfile.gpu -t scrolltech/cuda-testnet-runner .

gpu-asset-download:
	mkdir -p ./assets
	wget https://circuit-release.s3.us-west-2.amazonaws.com/setup/params20 -O ./assets/params20
	wget https://circuit-release.s3.us-west-2.amazonaws.com/setup/params24 -O ./assets/params24
	wget https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.7.2/layer1.config -O ./assets/layer1.config
	wget https://circuit-release.s3.us-west-2.amazonaws.com/release-v0.7.2/layer2.config -O ./assets/layer2.config
